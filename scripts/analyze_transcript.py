#!/usr/bin/env python3
"""Squeez transcript analyzer.

Quantitative pass over a Claude Code JSONL session file. Flags *candidates* for the
qualitative review — it does not draw conclusions.

Usage:
    python3 analyze_transcript.py session.jsonl [--out analysis.json] [--big-result-tokens 10000] [--sidechain]

Options:
    --sidechain    Include isSidechain=true records (use when analyzing a standalone
                   subagent file where all records carry isSidechain=true).
"""

import argparse
import json
import sys
from collections import defaultdict

CHARS_PER_TOKEN = 4  # rough estimate, only used for payload sizing


def est_tokens(obj) -> int:
    try:
        s = obj if isinstance(obj, str) else json.dumps(obj, ensure_ascii=False)
    except Exception:
        s = str(obj)
    return max(1, len(s) // CHARS_PER_TOKEN)


def load(path):
    records, malformed = [], []
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        for i, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                records.append((i, json.loads(line)))
            except json.JSONDecodeError as e:
                malformed.append({"line": i, "error": str(e)[:200]})
    return records, malformed


def content_blocks(rec):
    msg = rec.get("message") or {}
    c = msg.get("content")
    if isinstance(c, str):
        return [{"type": "text", "text": c}]
    return c if isinstance(c, list) else []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--out", default=None)
    ap.add_argument("--big-result-tokens", type=int, default=10_000)
    ap.add_argument("--sidechain", action="store_true",
                    help="Include isSidechain=true records (for standalone subagent files)")
    args = ap.parse_args()

    records, malformed = load(args.path)
    if not records:
        print("No parseable records found.", file=sys.stderr)
        sys.exit(1)

    timeline = []            # per assistant turn usage
    tool_calls = {}          # tool_use_id -> {name, input, line}
    call_signatures = defaultdict(list)   # (name, canonical input) -> [line,...]
    file_reads = defaultdict(list)        # file_path -> [line,...]
    file_edits = defaultdict(list)        # file_path -> [line,...]
    big_results = []
    errors = []
    compactions = []
    unmatched_results = []
    result_bytes_by_tool = defaultdict(int)
    sidechain_lines = 0
    types_seen = defaultdict(int)
    first_ts = last_ts = None

    for line_no, rec in records:
        rtype = rec.get("type", "?")
        types_seen[rtype] += 1
        ts = rec.get("timestamp")
        if ts:
            first_ts = first_ts or ts
            last_ts = ts
        if not args.sidechain and rec.get("isSidechain"):
            sidechain_lines += 1
            continue
        if rtype == "summary" or rec.get("isCompactSummary"):
            compactions.append({"line": line_no, "timestamp": ts})

        usage = (rec.get("message") or {}).get("usage")
        if rtype == "assistant" and isinstance(usage, dict):
            timeline.append({
                "line": line_no,
                "timestamp": ts,
                "input": usage.get("input_tokens", 0),
                "cache_read": usage.get("cache_read_input_tokens", 0),
                "cache_creation": usage.get("cache_creation_input_tokens", 0),
                "output": usage.get("output_tokens", 0),
                "model": (rec.get("message") or {}).get("model"),
            })

        for b in content_blocks(rec):
            btype = b.get("type")
            if btype == "tool_use":
                name = b.get("name", "?")
                binput = b.get("input") or {}
                tool_calls[b.get("id")] = {"name": name, "line": line_no}
                sig = (name, json.dumps(binput, sort_keys=True, ensure_ascii=False)[:500])
                call_signatures[sig].append(line_no)
                fp = binput.get("file_path") or binput.get("path")
                lname = name.lower()
                if fp:
                    if "read" in lname or "view" in lname:
                        file_reads[fp].append(line_no)
                    elif "edit" in lname or "write" in lname or "replace" in lname or "create" in lname:
                        file_edits[fp].append(line_no)
            elif btype == "tool_result":
                tid = b.get("tool_use_id")
                call = tool_calls.get(tid)
                if call is None:
                    unmatched_results.append({"line": line_no, "tool_use_id": tid})
                size = est_tokens(b.get("content", ""))
                tool_name = call["name"] if call else "unknown"
                result_bytes_by_tool[tool_name] += size
                if size >= args.big_result_tokens:
                    big_results.append({"line": line_no, "tool": tool_name,
                                        "est_tokens": size})
                if b.get("is_error"):
                    errors.append({"line": line_no, "tool": tool_name,
                                   "preview": str(b.get("content"))[:200]})

    # tool_use without any result
    answered = set()
    for line_no, rec in records:
        for b in content_blocks(rec):
            if b.get("type") == "tool_result":
                answered.add(b.get("tool_use_id"))
    unanswered_calls = [{"tool_use_id": tid, **info}
                        for tid, info in tool_calls.items() if tid not in answered]

    # cache invalidation candidates: cache_read drops >80% while cache_creation spikes
    invalidations = []
    for prev, cur in zip(timeline, timeline[1:]):
        if prev["cache_read"] > 5000 and cur["cache_read"] < prev["cache_read"] * 0.2 \
           and cur["cache_creation"] > prev["cache_read"] * 0.5:
            invalidations.append({"line": cur["line"], "timestamp": cur["timestamp"],
                                  "cache_read_before": prev["cache_read"],
                                  "cache_creation_after": cur["cache_creation"],
                                  "model_changed": prev["model"] != cur["model"]})

    # duplicate reads with no intervening edit
    dup_reads = []
    for fp, lines in file_reads.items():
        if len(lines) < 2:
            continue
        edits = sorted(file_edits.get(fp, []))
        redundant = [b for a, b in zip(lines, lines[1:])
                     if not any(a < e < b for e in edits)]
        if redundant:
            dup_reads.append({"file": fp, "all_read_lines": lines,
                              "redundant_read_lines": redundant})

    repeated_calls = [{"tool": sig[0], "lines": lines, "count": len(lines)}
                      for sig, lines in call_signatures.items() if len(lines) >= 2]

    totals = {
        "assistant_turns": len(timeline),
        "input_tokens": sum(t["input"] for t in timeline),
        "cache_read_tokens": sum(t["cache_read"] for t in timeline),
        "cache_creation_tokens": sum(t["cache_creation"] for t in timeline),
        "output_tokens": sum(t["output"] for t in timeline),
    }
    denom = totals["cache_read_tokens"] + totals["cache_creation_tokens"] + totals["input_tokens"]
    totals["cache_hit_rate"] = round(totals["cache_read_tokens"] / denom, 3) if denom else None

    report = {
        "file": args.path,
        "session_span": {"first": first_ts, "last": last_ts},
        "record_types": dict(types_seen),
        "malformed_lines": malformed,
        "sidechain_lines_skipped": sidechain_lines,
        "totals": totals,
        "timeline": timeline,
        "cache_invalidation_candidates": invalidations,
        "compaction_events": compactions,
        "duplicate_reads": sorted(dup_reads, key=lambda d: -len(d["redundant_read_lines"])),
        "repeated_identical_calls": sorted(repeated_calls, key=lambda d: -d["count"])[:30],
        "big_tool_results": sorted(big_results, key=lambda d: -d["est_tokens"])[:30],
        "tool_errors": errors,
        "result_est_tokens_by_tool": dict(sorted(result_bytes_by_tool.items(),
                                                 key=lambda kv: -kv[1])),
        "broken_pairing": {"unmatched_results": unmatched_results,
                           "unanswered_tool_calls": unanswered_calls},
        "notes": "Payload sizes are estimates (~4 chars/token). usage figures are measured.",
    }

    out = json.dumps(report, indent=2, ensure_ascii=False)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(out)
        print(f"Wrote {args.out}")
    else:
        print(out)


if __name__ == "__main__":
    main()
