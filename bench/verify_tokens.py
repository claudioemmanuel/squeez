#!/usr/bin/env python3
"""
verify_tokens.py — independent, real-tokenizer veracity check for squeez.

The Rust benchmark reports reduction using a `chars/4` token model for
reproducibility. A skeptic's fair objection: "chars/4 is a made-up unit — show
me the reduction under a *real* byte-pair tokenizer." This script does exactly
that. For every fixture it:

  1. runs squeez the same way bench/run.sh does (filter or compress-md),
  2. tokenizes BEFORE and AFTER with tiktoken `cl100k_base` (real GPT-4 BPE),
  3. also computes the legacy chars/4 estimate,

then prints per-fixture reduction under both models plus an aggregate. If the
real-BPE reduction tracks the chars/4 reduction, the headline claim is not an
artifact of the crude token unit — it is real.

Requires: pip install tiktoken
Usage:    python3 bench/verify_tokens.py [--json]
"""
import json
import subprocess
import sys
from pathlib import Path

try:
    import tiktoken
except ImportError:
    print("ERROR: pip install tiktoken", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "bench" / "fixtures"
SQUEEZ = ROOT / "target" / "release" / "squeez"

# cl100k_base is a real production BPE tokenizer (GPT-4 / GPT-3.5). It is not
# Claude's exact vocab, but reduction *ratios* are near model-invariant, so it
# is a legitimate independent ground truth for "did the token count really drop".
ENC = tiktoken.get_encoding("cl100k_base")

# Fixtures exercised by the wrap/context engine (summarize, cross-call dedup),
# not by single-pass filter — excluded from this deterministic filter check and
# verified separately by `squeez benchmark`.
SKIP = {
    "context_crosscall_1.txt", "context_crosscall_2.txt", "context_crosscall_3.txt",
    "summarize_huge.txt",  # wrap-mode summarize, not filter
}


def bpe(s: str) -> int:
    return len(ENC.encode(s, disallowed_special=()))


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


def main():
    as_json = "--json" in sys.argv
    if not SQUEEZ.exists():
        print(f"ERROR: {SQUEEZ} not found — run 'cargo build --release'", file=sys.stderr)
        sys.exit(1)

    rows = []
    tot_bpe_b = tot_bpe_a = tot_c4_b = tot_c4_a = 0
    for path in sorted(FIXTURES.glob("*.txt")):
        if path.name in SKIP:
            continue
        before = path.read_text(errors="replace")
        after = run_squeez(path)

        bb, ba = bpe(before), bpe(after)
        cb, ca = chars4(before), chars4(after)
        if bb == 0:
            continue
        tot_bpe_b += bb; tot_bpe_a += ba
        tot_c4_b += cb; tot_c4_a += ca
        rows.append({
            "fixture": path.name,
            "bpe_before": bb, "bpe_after": ba,
            "bpe_reduction_pct": round(100 * (1 - ba / bb), 1),
            "chars4_reduction_pct": round(100 * (1 - ca / cb), 1) if cb else 0.0,
        })

    agg_bpe = round(100 * (1 - tot_bpe_a / tot_bpe_b), 1)
    agg_c4 = round(100 * (1 - tot_c4_a / tot_c4_b), 1)
    result = {
        "tokenizer": "cl100k_base (real BPE)",
        "fixtures": len(rows),
        "total_bpe_before": tot_bpe_b, "total_bpe_after": tot_bpe_a,
        "aggregate_bpe_reduction_pct": agg_bpe,
        "aggregate_chars4_reduction_pct": agg_c4,
        "delta_pct": round(abs(agg_bpe - agg_c4), 1),
        "rows": rows,
    }

    if as_json:
        print(json.dumps(result, indent=2))
        return

    print("=" * 74)
    print("  squeez veracity check — reduction under a REAL BPE tokenizer")
    print("  tokenizer: cl100k_base (GPT-4 family, tiktoken)")
    print("=" * 74)
    print(f"  {'FIXTURE':<32}{'BPE tk':>10}{'→':>4}{'after':>8}{'  real%':>8}{' chars/4%':>10}")
    print("  " + "-" * 70)
    for r in rows:
        print(f"  {r['fixture']:<32}{r['bpe_before']:>10}{'→':>4}{r['bpe_after']:>8}"
              f"{r['bpe_reduction_pct']:>7}%{r['chars4_reduction_pct']:>9}%")
    print("  " + "-" * 70)
    print(f"  {'AGGREGATE (' + str(len(rows)) + ' fixtures)':<32}"
          f"{tot_bpe_b:>10}{'→':>4}{tot_bpe_a:>8}{agg_bpe:>7}%{agg_c4:>9}%")
    print("=" * 74)
    print(f"  Real-BPE reduction:   {agg_bpe}%")
    print(f"  chars/4 reduction:    {agg_c4}%")
    print(f"  Divergence:           {result['delta_pct']} pts "
          f"→ {'CONFIRMED — claim is not a token-model artifact' if result['delta_pct'] <= 5 else 'REVIEW — models diverge'}")
    print("=" * 74)


if __name__ == "__main__":
    main()
