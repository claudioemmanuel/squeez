FIXTURE                               BEFORE    AFTER  REDUCTION  LATENCY STATUS
──────────────────────────────────────────────────────────────────────────────
docker_logs.txt                         665tk     186tk        72%      49µs  ✅
env_dump.txt                            441tk     287tk        35%      28µs  ✅
find_deep.txt                           424tk     134tk        68%      68µs  ✅
git_copilot_session.txt                 640tk     421tk        34%      96µs  ✅
git_diff.txt                            502tk     497tk         1%      53µs  ✅
git_log_200.txt                        2692tk     289tk        89%     204µs  ✅
git_status.txt                           50tk      16tk        68%      23µs  ✅
intensity_budget80.txt                 4418tk     256tk        94%      47µs  ✅
ls_la.txt                              1782tk     886tk        50%     120µs  ✅
mdcompress_claude_md.txt                316tk     247tk        22%     213µs  ✅
mdcompress_prose.txt                    187tk     139tk        26%     161µs  ✅
npm_install.txt                         524tk     232tk        56%      42µs  ✅
ps_aux.txt                            40373tk    2352tk        94%     1.8ms  ✅

PASS: 13/13  FAIL: 0/13

Additional synthetic scenarios (squeez benchmark):
cargo_build.txt                        2106tk     452tk        79%     213µs  ✅
tsc_errors.txt                          731tk     101tk        86%      57µs  ✅
verbose_log.txt                        4957tk    1991tk        60%     272µs  ✅
repetitive.txt                         4692tk      37tk        99%     239µs  ✅
kubectl_pods.txt                       1513tk    1513tk         0%      45µs  ✅
summarize_huge (wrap)                 82257tk     420tk        99%      63ms  ✅
crosscall_3x (wrap/dedup)               486tk     241tk        50%      58ms  ✅

TOTAL: 19/19  FAIL: 0/19

Aggregate: 145,338 tk → 10,441 tk  (-92.8%)
Cost savings: 92.8% (Claude Sonnet 4.6 · $3.00/MTok input)
Quality: 19/19 pass (signal terms preserved)

Run: squeez benchmark --json --output bench/benchmark_report.json
