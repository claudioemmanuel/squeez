FIXTURE                               BEFORE    AFTER  REDUCTION  LATENCY STATUS
──────────────────────────────────────────────────────────────────────────────
cat_claude_md.txt                     18406tk    2382tk        88%       6ms  ✅
docker_logs.txt                         665tk     180tk        73%       5ms  ✅
env_dump.txt                            441tk     287tk        35%       5ms  ✅
find_deep.txt                           424tk     279tk        35%       5ms  ✅
git_copilot_session.txt                 639tk     421tk        35%       5ms  ✅
git_diff.txt                            502tk     317tk        37%       5ms  ✅
git_log_200.txt                        2667tk     806tk        70%       6ms  ✅
git_status.txt                           50tk      16tk        68%       5ms  ✅
intensity_budget80.txt                 4418tk     728tk        84%       6ms  ✅
ls_la.txt                              1782tk     872tk        52%       5ms  ✅
mdcompress_claude_md.txt                316tk     246tk        23%       5ms  ✅
mdcompress_en_prose.txt                 514tk     434tk        16%       6ms  ✅
mdcompress_prose.txt                    187tk     138tk        27%       5ms  ✅
mdcompress_ptbr_prose.txt               540tk     455tk        16%       5ms  ✅
npm_install.txt                         524tk     218tk        59%       5ms  ✅
ps_aux.txt                            40373tk    2338tk        95%       8ms  ✅
summarize_huge.txt                    82257tk    1930tk        98%      26ms  ✅
xcodebuild_build.txt                   1881tk      17tk       100%       5ms  ✅

PASS: 18/18  FAIL: 0/18
