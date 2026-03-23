#!/usr/bin/env bash
# Usage: bash bench/capture.sh "git log --oneline -200" > bench/fixtures/git_log_200.txt
eval "$@" 2>&1
