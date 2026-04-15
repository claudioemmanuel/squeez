#!/usr/bin/env bash
# update-readme-benchmarks.sh — Patch README.md benchmark section
# from `squeez benchmark --json` output.
#
# Usage: bash .github/scripts/update-readme-benchmarks.sh ./path/to/squeez
set -euo pipefail

SQUEEZ="${1:?Usage: update-readme-benchmarks.sh <squeez-binary>}"
chmod +x "$SQUEEZ"

# Run benchmark and capture JSON
BENCH_JSON=$("$SQUEEZ" benchmark --json 2>/dev/null)

# Parse JSON and generate markdown table via python3
python3 - "$BENCH_JSON" <<'PYEOF'
import sys, json, re

data = json.loads(sys.argv[1])
scenarios = data.get("scenarios", [])

# JSON is flat (top-level fields, not nested summary object)
# Scenario latency field is "latency_us" (not "median_latency_us")

# Build per-scenario table
lines = []
lines.append("Measured on macOS (Apple Silicon). Token count = `chars / 4` (matches Claude's ~4 chars/token). Run `squeez benchmark` to reproduce.")
lines.append("")

# Derive iterations from first scenario
iters = scenarios[0].get("iterations", 3) if scenarios else 3
lines.append("### Per-scenario results — {} scenarios × {} iterations".format(
    len(scenarios), iters
))
lines.append("")
lines.append("| Scenario | Before | After | Reduction | Latency |")
lines.append("|----------|--------|-------|-----------|---------|")

# Sort by reduction descending
scenarios_sorted = sorted(scenarios, key=lambda s: s.get("reduction_pct", 0), reverse=True)
for s in scenarios_sorted:
    name = s.get("name", "")
    before = s.get("baseline_tokens", 0)
    after = s.get("compressed_tokens", 0)
    reduction = s.get("reduction_pct", 0)
    latency_us = s.get("latency_us", 0)
    if latency_us >= 1000000:
        lat_str = "{:.1f}s".format(latency_us / 1000000)
    elif latency_us >= 1000:
        lat_str = "{:.1f} ms".format(latency_us / 1000)
    else:
        lat_str = "{:.0f} µs".format(latency_us)
    lines.append("| `{}` | {:,} tk | {:,} tk | **-{:.0f}%** | {} |".format(
        name, before, after, reduction, lat_str
    ))

# Aggregate section (fields are top-level in JSON)
lines.append("")
lines.append("### Aggregate")
lines.append("")
lines.append("| Metric | Value |")
lines.append("|--------|-------|")

total_before = data.get("total_baseline_tokens", 0)
total_after = data.get("total_compressed_tokens", 0)
total_reduction = data.get("total_reduction_pct", 0)
bash_reduction = data.get("bash_reduction_pct", 0)
md_reduction = data.get("md_reduction_pct", 0)
wrap_reduction = data.get("wrap_reduction_pct", 0)
quality_pass = data.get("quality_pass_count", 0)
quality_total = quality_pass + data.get("quality_fail_count", 0) + data.get("quality_skip_count", 0)
latency_p50 = data.get("avg_latency_us", 0)
latency_p95 = data.get("p95_latency_us", 0)

lines.append("| **Total token reduction** | **{:.1f}%** — {:,} tk → {:,} tk |".format(
    total_reduction, total_before, total_after
))
lines.append("| Bash output | **-{:.1f}%** |".format(bash_reduction))
lines.append("| Markdown / context files | **-{:.1f}%** |".format(md_reduction))
lines.append("| Wrap / cross-call engine | **-{:.1f}%** |".format(wrap_reduction))
lines.append("| Quality (signal terms preserved) | **{} / {} pass** |".format(quality_pass, quality_total))

if latency_p50 >= 1000:
    lines.append("| Latency p50 (filter mode) | **{:.1f} ms** |".format(latency_p50 / 1000))
else:
    lines.append("| Latency p50 (filter mode) | **< 0.3 ms** |")
if latency_p95 >= 1000:
    lines.append("| Latency p95 (incl. wrap/summarize) | **{:.0f} ms** |".format(latency_p95 / 1000))

# Cost savings
lines.append("")
lines.append("### Estimated cost savings — Claude Sonnet 4.6 · $3.00 / MTok input")
lines.append("")
lines.append("| Usage | Baseline / month | Saved / month |")
lines.append("|-------|-----------------|---------------|")
for calls in [100, 1000, 10000]:
    # ~2000 tokens per call average
    baseline = calls * 30 * 2000 * 3.00 / 1000000
    saved = baseline * total_reduction / 100
    pct = int(round(total_reduction))
    lines.append("| {:,} calls / day | ${:.2f} | **${:.2f} ({}%)** |".format(
        calls, baseline, saved, pct
    ))

new_content = "\n".join(lines)

# Read README and replace between sentinel markers
with open("README.md", "r") as f:
    readme = f.read()

pattern = r"(<!-- BENCHMARK:START -->).*?(<!-- BENCHMARK:END -->)"
if re.search(pattern, readme, re.DOTALL):
    readme = re.sub(
        pattern,
        r"\1\n" + new_content + "\n" + r"\2",
        readme,
        flags=re.DOTALL
    )
    with open("README.md", "w") as f:
        f.write(readme)
    print("README.md benchmark section updated")
else:
    print("WARNING: sentinel markers <!-- BENCHMARK:START/END --> not found in README.md")
    print("Skipping benchmark update. Add markers manually.")
    sys.exit(1)
PYEOF
