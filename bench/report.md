FIXTURE                               BEFORE    AFTER  REDUCTION  LATENCY STATUS
──────────────────────────────────────────────────────────────────────────────
bundle_install.txt                      121tk      28tk        77%       8ms  ✅
cat_claude_md.txt                     18406tk    2382tk        88%       9ms  ✅
docker_logs.txt                         665tk     180tk        73%       7ms  ✅
env_dump.txt                            441tk     287tk        35%       7ms  ✅
find_deep.txt                           424tk     279tk        35%       7ms  ✅
git_copilot_session.txt                 639tk     421tk        35%       7ms  ✅
git_diff.txt                            502tk     317tk        37%       7ms  ✅
git_log_200.txt                        2667tk     806tk        70%       7ms  ✅
git_status.txt                           50tk      16tk        68%       8ms  ✅
intensity_budget80.txt                 4418tk     728tk        84%      11ms  ✅
ls_la.txt                              1782tk     872tk        52%       9ms  ✅
mdcompress_claude_md.txt                316tk     270tk        15%       7ms  ✅
mdcompress_en_prose.txt                 514tk     445tk        14%       8ms  ✅
mdcompress_prose.txt                    187tk     141tk        25%       9ms  ✅
mdcompress_ptbr_prose.txt               540tk     463tk        15%       8ms  ✅
mypy_errors.txt                         650tk     349tk        47%       9ms  ✅
npm_install.txt                         524tk     218tk        59%       7ms  ✅
pip_install.txt                         406tk      62tk        85%       7ms  ✅
ps_aux.txt                            40373tk    2338tk        95%      12ms  ✅
rsync_transfer.txt                      912tk      26tk        98%       8ms  ✅
shellcheck_findings.txt                 335tk     187tk        45%       8ms  ✅
summarize_huge.txt                    82257tk    1930tk        98%      48ms  ✅
systemctl_status.txt                    727tk      41tk        95%       7ms  ✅
xcodebuild_build.txt                   1881tk      17tk       100%       8ms  ✅

PASS: 24/24  FAIL: 0/24
