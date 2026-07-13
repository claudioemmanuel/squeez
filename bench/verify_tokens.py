#!/usr/bin/env python3
"""
verify_tokens.py — independent, real-tokenizer veracity check for squeez.

The Rust benchmark reports reduction using a `chars/4` token model for
reproducibility. A skeptic's fair objection: "chars/4 is a made-up unit — show
me the reduction under a *real* byte-pair tokenizer." This script does exactly
that. For every fixture it:

  1. runs squeez the same way bench/run.sh does (filter or compress-md),
  2. tokenizes BEFORE and AFTER with tiktoken `cl100k_base` (real GPT-4 BPE)
     and, when available locally, `o200k_base` (real GPT-4o BPE),
  3. also computes the legacy chars/4 estimate,

then prints per-fixture reduction under both models plus an aggregate. If the
real-BPE reduction tracks the chars/4 reduction, the headline claim is not an
artifact of the crude token unit — it is real.

Two additional audit modes (see --help for full docs):
  --marginal        3-arm harness: raw vs naive tail-truncation control vs
                     squeez, isolating squeez's MARGINAL savings over a
                     length-matched naive baseline.
  --substitutions    audits whether squeez's ultra-persona word substitutions
                     (with→w/, because→b/c, ...) actually save BPE tokens.

Requires: pip install tiktoken
Usage:    python3 bench/verify_tokens.py [--json | --marginal | --substitutions | --help]
"""
import json
import re
import statistics
import subprocess
import sys
from pathlib import Path

try:
    import tiktoken
except ImportError:
    print("ERROR: tiktoken is not installed. Run: pip install tiktoken", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "bench" / "fixtures"
SQUEEZ = ROOT / "target" / "release" / "squeez"

# cl100k_base is a real production BPE tokenizer (GPT-4 / GPT-3.5). It is not
# Claude's exact vocab, but reduction *ratios* are near model-invariant, so it
# is a legitimate independent ground truth for "did the token count really drop".
ENC_CL100K = tiktoken.get_encoding("cl100k_base")

# o200k_base (GPT-4o family) is a second, newer real BPE vocab. Reporting both
# shows the reduction claim isn't an artifact of picking one tokenizer.
try:
    ENC_O200K = tiktoken.get_encoding("o200k_base")
    HAS_O200K = True
except Exception as exc:  # local tiktoken install may lack this encoding's cached BPE ranks
    ENC_O200K = None
    HAS_O200K = False
    print(f"WARN: o200k_base unavailable locally ({exc}); reporting cl100k_base only", file=sys.stderr)

# Fixtures exercised by the wrap/context engine (summarize, cross-call dedup),
# not by single-pass filter — excluded from this deterministic filter check and
# verified separately by `squeez benchmark`.
SKIP = {
    "context_crosscall_1.txt", "context_crosscall_2.txt", "context_crosscall_3.txt",
    "summarize_huge.txt",  # wrap-mode summarize, not filter
}

HELP = """\
verify_tokens.py — real-tokenizer veracity + marginal-savings + substitution audits for squeez

Usage:
  python3 bench/verify_tokens.py                 # default veracity check (chars/4 vs real BPE)
  python3 bench/verify_tokens.py --json           # ...as JSON
  python3 bench/verify_tokens.py --marginal       # 3-arm marginal-savings audit
  python3 bench/verify_tokens.py --marginal --json
  python3 bench/verify_tokens.py --substitutions  # word-substitution BPE audit
  python3 bench/verify_tokens.py --substitutions --json
  python3 bench/verify_tokens.py --help           # this message

Tokenizers: cl100k_base (GPT-4 family) is always used. o200k_base (GPT-4o
family) is reported alongside it whenever the local tiktoken install has that
encoding's BPE ranks available; otherwise that column/field is dropped and a
warning is printed to stderr.

--marginal
  For each fixture in bench/fixtures/ (excluding the wrap-mode-only fixtures
  in SKIP), three arms are compared:
    A = raw fixture text, unmodified.
    B = naive control — tail -N truncation of the raw fixture, where N is
        chosen to equal squeez's own OUTPUT LINE COUNT for that fixture (arm
        C below). Rule: N = len(C.splitlines()). This is the fairness rule —
        a naive "just truncate it" baseline is only a fair comparison if it's
        judged at the same output length squeez actually produced, not some
        arbitrary N.
    C = squeez-filtered output, produced the same way bench/run.sh invokes
        squeez: `squeez filter <hint>` for plain-text fixtures (hint = the
        part of the filename before the first "_"), or
        `squeez compress-md --dry-run --ultra --quiet [--lang pt-BR]` for
        mdcompress_* fixtures.
  squeez's MARGINAL delta = B - C: tokens squeez saves beyond what naive
  length-matched truncation would already save "for free". Per fixture this
  script prints A/B/C token counts under both encodings plus the delta, then
  reports median/mean/min/max/stdev of the delta across all fixtures.
  A deterministic snapshot is written to bench/marginal_snapshot.json.

--substitutions
  Audits whether squeez's ultra-persona word substitutions actually reduce
  BPE token counts. Pairs are parsed from:
    - src/commands/compress_md/locales/en.rs    (`ultra_subs`, English)
    - src/commands/compress_md/locales/pt_br.rs (`ultra_subs`, Portuguese)
    - assets/persona_ultra.md                   (the "Substitutions:" line)
  Duplicate (from, to) pairs across sources are merged into one row.
  Each pair is embedded — both the long form and its abbreviation — in 3
  sentence contexts (mid-sentence, start-of-sentence, end-of-sentence) and
  tokenized with both encodings. A context "wins" if the substitution
  reduces the combined token count in that context. Verdict is SAVES when
  both encodings agree the mean delta is positive, COSTS when both agree
  it's negative, otherwise NEUTRAL. A deterministic snapshot is written to
  bench/substitutions_snapshot.json, sorted with COSTS/NEUTRAL pairs first.
"""


def bpe(s: str, enc) -> int:
    return len(enc.encode(s, disallowed_special=()))


def chars4(s: str) -> int:
    return len(s) // 4


def run_squeez(path: Path) -> str:
    name = path.name
    text = path.read_text(errors="replace")
    hint = name.split("_")[0]
    if hint == "mdcompress":
        lang = []
        low = name.lower()
        if "ptbr" in low or "pt_br" in low or "pt-br" in low:
            lang = ["--lang", "pt-BR"]
        try:
            out = subprocess.run(
                [str(SQUEEZ), "compress-md", "--dry-run", "--ultra", "--quiet", *lang, str(path)],
                capture_output=True, text=True, timeout=30,
            )
            return out.stdout if out.returncode == 0 and out.stdout else text
        except Exception:
            return text
    else:
        try:
            out = subprocess.run(
                [str(SQUEEZ), "filter", hint],
                input=text, capture_output=True, text=True, timeout=30,
            )
            return out.stdout if out.returncode == 0 and out.stdout else text
        except Exception:
            return text


# ---------------------------------------------------------------------------
# Default veracity check
# ---------------------------------------------------------------------------

def run_veracity(as_json: bool):
    if not SQUEEZ.exists():
        print(f"ERROR: {SQUEEZ} not found — run 'cargo build --release'", file=sys.stderr)
        sys.exit(1)

    rows = []
    tot_bpe_b = tot_bpe_a = tot_c4_b = tot_c4_a = 0
    tot_o2_b = tot_o2_a = 0
    for path in sorted(FIXTURES.glob("*.txt")):
        if path.name in SKIP:
            continue
        before = path.read_text(errors="replace")
        after = run_squeez(path)

        bb, ba = bpe(before, ENC_CL100K), bpe(after, ENC_CL100K)
        cb, ca = chars4(before), chars4(after)
        if bb == 0:
            continue
        tot_bpe_b += bb; tot_bpe_a += ba
        tot_c4_b += cb; tot_c4_a += ca
        row = {
            "fixture": path.name,
            "bpe_before": bb, "bpe_after": ba,
            "bpe_reduction_pct": round(100 * (1 - ba / bb), 1),
            "chars4_reduction_pct": round(100 * (1 - ca / cb), 1) if cb else 0.0,
        }
        if HAS_O200K:
            ob, oa = bpe(before, ENC_O200K), bpe(after, ENC_O200K)
            tot_o2_b += ob; tot_o2_a += oa
            row["o200k_before"] = ob
            row["o200k_after"] = oa
            row["o200k_reduction_pct"] = round(100 * (1 - oa / ob), 1) if ob else 0.0
        rows.append(row)

    agg_bpe = round(100 * (1 - tot_bpe_a / tot_bpe_b), 1)
    agg_c4 = round(100 * (1 - tot_c4_a / tot_c4_b), 1)
    result = {
        "tokenizer": "cl100k_base (real BPE)",
        "tokenizer_secondary": "o200k_base (real BPE)" if HAS_O200K else None,
        "fixtures": len(rows),
        "total_bpe_before": tot_bpe_b, "total_bpe_after": tot_bpe_a,
        "aggregate_bpe_reduction_pct": agg_bpe,
        "aggregate_chars4_reduction_pct": agg_c4,
        "delta_pct": round(abs(agg_bpe - agg_c4), 1),
        "rows": rows,
    }
    if HAS_O200K and tot_o2_b:
        agg_o2 = round(100 * (1 - tot_o2_a / tot_o2_b), 1)
        result["total_o200k_before"] = tot_o2_b
        result["total_o200k_after"] = tot_o2_a
        result["aggregate_o200k_reduction_pct"] = agg_o2

    if as_json:
        print(json.dumps(result, indent=2))
        return

    print("=" * 88)
    print("  squeez veracity check — reduction under REAL BPE tokenizers")
    print("  tokenizers: cl100k_base (GPT-4)" + (" + o200k_base (GPT-4o)" if HAS_O200K else " (o200k_base unavailable)"))
    print("=" * 88)
    header = f"  {'FIXTURE':<32}{'BPE tk':>10}{'→':>4}{'after':>8}{'  real%':>8}{' chars/4%':>10}"
    if HAS_O200K:
        header += f"{'  o200k%':>10}"
    print(header)
    print("  " + "-" * (70 + (10 if HAS_O200K else 0)))
    for r in rows:
        line = (f"  {r['fixture']:<32}{r['bpe_before']:>10}{'→':>4}{r['bpe_after']:>8}"
                f"{r['bpe_reduction_pct']:>7}%{r['chars4_reduction_pct']:>9}%")
        if HAS_O200K:
            line += f"{r['o200k_reduction_pct']:>9}%"
        print(line)
    print("  " + "-" * (70 + (10 if HAS_O200K else 0)))
    footer = (f"  {'AGGREGATE (' + str(len(rows)) + ' fixtures)':<32}"
              f"{tot_bpe_b:>10}{'→':>4}{tot_bpe_a:>8}{agg_bpe:>7}%{agg_c4:>9}%")
    if HAS_O200K:
        footer += f"{result.get('aggregate_o200k_reduction_pct', 0.0):>9}%"
    print(footer)
    print("=" * 88)
    print(f"  Real-BPE reduction (cl100k_base): {agg_bpe}%")
    if HAS_O200K:
        print(f"  Real-BPE reduction (o200k_base):  {result.get('aggregate_o200k_reduction_pct', 0.0)}%")
    print(f"  chars/4 reduction:                 {agg_c4}%")
    print(f"  Divergence (cl100k_base vs chars/4): {result['delta_pct']} pts "
          f"→ {'CONFIRMED — claim is not a token-model artifact' if result['delta_pct'] <= 5 else 'REVIEW — models diverge'}")
    print("=" * 88)


# ---------------------------------------------------------------------------
# --marginal: 3-arm harness (raw vs naive tail-truncation vs squeez)
# ---------------------------------------------------------------------------

def naive_tail_truncate(text: str, n_lines: int) -> str:
    lines = text.splitlines()
    if n_lines <= 0:
        return ""
    if n_lines >= len(lines):
        return text
    return "\n".join(lines[-n_lines:])


def stats_for(deltas):
    if not deltas:
        return {"median": 0.0, "mean": 0.0, "min": 0, "max": 0, "stdev": 0.0}
    return {
        "median": statistics.median(deltas),
        "mean": round(statistics.mean(deltas), 2),
        "min": min(deltas),
        "max": max(deltas),
        "stdev": round(statistics.pstdev(deltas), 2) if len(deltas) > 1 else 0.0,
    }


def run_marginal(as_json: bool):
    if not SQUEEZ.exists():
        print(f"ERROR: {SQUEEZ} not found — run 'cargo build --release'", file=sys.stderr)
        sys.exit(1)

    rows = []
    for path in sorted(FIXTURES.glob("*.txt")):
        if path.name in SKIP:
            continue
        raw = path.read_text(errors="replace")
        if not raw:
            continue
        squeezed = run_squeez(path)  # arm C
        n = len(squeezed.splitlines())
        control = naive_tail_truncate(raw, n)  # arm B, length-matched to C

        a_cl, b_cl, c_cl = bpe(raw, ENC_CL100K), bpe(control, ENC_CL100K), bpe(squeezed, ENC_CL100K)
        row = {
            "fixture": path.name,
            "control_lines": n,
            "cl100k": {"A": a_cl, "B": b_cl, "C": c_cl, "marginal_delta": b_cl - c_cl},
        }
        if HAS_O200K:
            a_o2, b_o2, c_o2 = bpe(raw, ENC_O200K), bpe(control, ENC_O200K), bpe(squeezed, ENC_O200K)
            row["o200k"] = {"A": a_o2, "B": b_o2, "C": c_o2, "marginal_delta": b_o2 - c_o2}
        rows.append(row)

    summary = {"cl100k": stats_for([r["cl100k"]["marginal_delta"] for r in rows])}
    if HAS_O200K:
        summary["o200k"] = stats_for([r["o200k"]["marginal_delta"] for r in rows])

    snapshot = {
        "control_rule": "tail -N truncation; N = squeez output line count (arm C) — length-matched for fairness",
        "fixtures": len(rows),
        "rows": rows,
        "summary": summary,
    }
    (ROOT / "bench" / "marginal_snapshot.json").write_text(json.dumps(snapshot, indent=2) + "\n")

    if as_json:
        print(json.dumps(snapshot, indent=2))
        return

    o2_on = HAS_O200K
    print("=" * 100)
    print("  squeez MARGINAL savings audit — A=raw, B=naive tail-truncation (len-matched), C=squeez")
    print("  fairness rule: B truncates to len(C) lines, so B and C are judged at the same output length")
    print("=" * 100)
    header = f"  {'FIXTURE':<32}{'cl100k A':>9}{'B':>6}{'C':>6}{'Δ(B-C)':>8}"
    if o2_on:
        header += f"   {'o200k A':>9}{'B':>6}{'C':>6}{'Δ(B-C)':>8}"
    print(header)
    print("  " + "-" * (61 + (36 if o2_on else 0)))
    for r in rows:
        cl = r["cl100k"]
        line = f"  {r['fixture']:<32}{cl['A']:>9}{cl['B']:>6}{cl['C']:>6}{cl['marginal_delta']:>+8}"
        if o2_on:
            o2 = r["o200k"]
            line += f"   {o2['A']:>9}{o2['B']:>6}{o2['C']:>6}{o2['marginal_delta']:>+8}"
        print(line)
    print("  " + "-" * (61 + (36 if o2_on else 0)))
    print(f"  MARGINAL delta (B-C) summary across {len(rows)} fixtures:")
    for enc_name, st in summary.items():
        print(f"    {enc_name:<8} median={st['median']:>8}  mean={st['mean']:>8}  "
              f"min={st['min']:>8}  max={st['max']:>8}  stdev={st['stdev']:>8}")
    print("=" * 100)


# ---------------------------------------------------------------------------
# --substitutions: does squeez's word-substitution list actually save tokens?
# ---------------------------------------------------------------------------

def parse_rust_ultra_subs(path: Path):
    src = path.read_text()
    start = src.index("ultra_subs")
    start = src.index("[", start)
    depth = 0
    end = start
    for i, ch in enumerate(src[start:], start):
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
            if depth == 0:
                end = i
                break
    block = src[start:end]
    return re.findall(r'\("((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"\)', block)


def parse_persona_pairs(path: Path):
    text = path.read_text()
    m = re.search(r"Substitutions:(.*?)\n\s*\n", text, re.S)
    if not m:
        return []
    chunk = m.group(1).replace("\n", " ").strip().rstrip(".")
    pairs = []
    for item in chunk.split(","):
        item = item.strip()
        if "→" in item:  # "→"
            f, t = item.split("→", 1)
            pairs.append((f.strip(), t.strip()))
    return pairs


def load_all_pairs():
    en_pairs = parse_rust_ultra_subs(ROOT / "src" / "commands" / "compress_md" / "locales" / "en.rs")
    ptbr_pairs = parse_rust_ultra_subs(ROOT / "src" / "commands" / "compress_md" / "locales" / "pt_br.rs")
    persona_pairs = parse_persona_pairs(ROOT / "assets" / "persona_ultra.md")

    merged = {}

    def add(pairs, source, lang):
        for f, t in pairs:
            key = (f, t)
            if key not in merged:
                merged[key] = {"lang": lang, "sources": []}
            if source not in merged[key]["sources"]:
                merged[key]["sources"].append(source)

    add(en_pairs, "en.rs", "en")
    add(persona_pairs, "persona_ultra.md", "en")
    add(ptbr_pairs, "pt_br.rs", "pt-BR")
    return merged


# 3 contexts per pair: mid-sentence, start-of-sentence, end-of-sentence.
# Position matters for BPE (leading space, sentence-initial capitalization,
# adjacency to punctuation all shift token boundaries).
EN_TEMPLATES = [
    lambda w: f"Check the diff {w} the new branch before merging.",
    lambda w: f"{w[:1].upper()}{w[1:]} that in place, the tests should pass now.",
    lambda w: f"We already covered that point {w}.",
]
PT_TEMPLATES = [
    lambda w: f"Veja o diff {w} o novo branch antes de aplicar merge.",
    lambda w: f"{w[:1].upper()}{w[1:]} isso definido, os testes devem passar agora.",
    lambda w: f"Já cobrimos esse ponto {w}.",
]


def compute_pair_row(frm, to, lang, sources):
    templates = EN_TEMPLATES if lang == "en" else PT_TEMPLATES
    deltas_cl, deltas_o2 = [], []
    for tmpl in templates:
        before, after = tmpl(frm), tmpl(to)
        deltas_cl.append(bpe(before, ENC_CL100K) - bpe(after, ENC_CL100K))
        if HAS_O200K:
            deltas_o2.append(bpe(before, ENC_O200K) - bpe(after, ENC_O200K))

    if HAS_O200K:
        wins = sum(1 for i in range(len(templates)) if deltas_cl[i] + deltas_o2[i] > 0)
    else:
        wins = sum(1 for d in deltas_cl if d > 0)

    mean_cl = round(statistics.mean(deltas_cl), 2)
    mean_o2 = round(statistics.mean(deltas_o2), 2) if HAS_O200K else None

    if mean_cl > 0 and (not HAS_O200K or mean_o2 > 0):
        verdict = "SAVES"
    elif mean_cl < 0 and (not HAS_O200K or mean_o2 < 0):
        verdict = "COSTS"
    else:
        verdict = "NEUTRAL"

    return {
        "from": frm, "to": to, "lang": lang, "sources": sources,
        "ctx_wins": f"{wins}/{len(templates)}",
        "mean_delta_cl100k": mean_cl,
        "mean_delta_o200k": mean_o2,
        "verdict": verdict,
    }


def run_substitutions(as_json: bool):
    merged = load_all_pairs()
    rows = [compute_pair_row(f, t, v["lang"], v["sources"]) for (f, t), v in merged.items()]

    rank = {"COSTS": 0, "NEUTRAL": 1, "SAVES": 2}
    rows.sort(key=lambda r: (rank[r["verdict"]], r["mean_delta_cl100k"], r["from"]))

    counts = {"SAVES": 0, "NEUTRAL": 0, "COSTS": 0}
    for r in rows:
        counts[r["verdict"]] += 1

    snapshot = {
        "tokenizer_primary": "cl100k_base",
        "tokenizer_secondary": "o200k_base" if HAS_O200K else None,
        "pair_count": len(rows),
        "verdict_counts": counts,
        "rows": rows,
    }
    (ROOT / "bench" / "substitutions_snapshot.json").write_text(json.dumps(snapshot, indent=2) + "\n")

    if as_json:
        print(json.dumps(snapshot, indent=2))
        return

    print("=" * 100)
    print("  squeez substitutions audit — do word substitutions actually save BPE tokens?")
    print("  encodings: cl100k_base" + (" + o200k_base" if HAS_O200K else " (o200k_base unavailable)"))
    print("  NOTE: losers (NEUTRAL/COSTS) sorted first")
    print("=" * 100)
    print(f"  {'PAIR':<26}{'LANG':<7}{'SOURCE':<24}{'CTX WINS':>9}{'Δcl100k':>10}{'Δo200k':>9}{'VERDICT':>9}")
    print("  " + "-" * 96)
    for r in rows:
        pair = f"{r['from']}→{r['to']}"
        src = ",".join(r["sources"])
        d_o2 = f"{r['mean_delta_o200k']:+.2f}" if r["mean_delta_o200k"] is not None else "n/a"
        print(f"  {pair:<26}{r['lang']:<7}{src:<24}{r['ctx_wins']:>9}"
              f"{r['mean_delta_cl100k']:>+10.2f}{d_o2:>9}{r['verdict']:>9}")
    print("  " + "-" * 96)
    print(f"  SAVES: {counts['SAVES']}   NEUTRAL: {counts['NEUTRAL']}   COSTS: {counts['COSTS']}   (total {len(rows)})")
    print("=" * 100)


def main():
    argv = sys.argv[1:]
    if "--help" in argv or "-h" in argv:
        print(HELP)
        return
    as_json = "--json" in argv
    if "--marginal" in argv:
        run_marginal(as_json)
        return
    if "--substitutions" in argv:
        run_substitutions(as_json)
        return
    run_veracity(as_json)


if __name__ == "__main__":
    main()
