FIXTURE                               BEFORE    AFTER  REDUCTION  LATENCY STATUS
──────────────────────────────────────────────────────────────────────────────
cat_claude_md.txt                     18407tk    2382tk        88%       8ms  ✅
docker_logs.txt                         665tk     181tk        73%       4ms  ✅
env_dump.txt                            441tk     287tk        35%       7ms  ✅
find_deep.txt                           424tk     279tk        35%       4ms  ✅
git_copilot_session.txt                 640tk     421tk        35%       9ms  ✅
git_diff.txt                            502tk     317tk        37%       8ms  ✅
git_log_200.txt                        2692tk     814tk        70%      11ms  ✅
git_status.txt                           50tk      16tk        68%       6ms  ✅
intensity_budget80.txt                 4418tk     729tk        84%       8ms  ✅
ls_la.txt                              1782tk     872tk        52%       8ms  ✅
mdcompress_claude_md.txt                316tk     270tk        15%       4ms  ✅
mdcompress_en_prose.txt                 514tk     445tk        14%       5ms  ✅
mdcompress_prose.txt                    187tk     141tk        25%       4ms  ✅
mdcompress_ptbr_prose.txt               558tk     479tk        15%       5ms  ✅
npm_install.txt                         524tk     218tk        59%       4ms  ✅
ps_aux.txt                            40373tk    2338tk        95%       7ms  ✅
summarize_huge.txt                    82257tk    1930tk        98%      24ms  ✅
xcodebuild_build.txt                   1881tk      17tk       100%       7ms  ✅

PASS: 18/18  FAIL: 0/18
