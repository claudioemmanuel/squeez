#!/usr/bin/env bash
# Layer 0 spike — does Claude Code's UserPromptExpansion hook let us REWRITE the
# expanded skill/slash-command body before the model sees it?
#
# This script is a throwaway probe. It:
#   1. logs the raw hook JSON it receives to spike/expansion-input.json
#   2. emits candidate output JSON trying every plausible rewrite field at once,
#      replacing the body with a unique sentinel, plus an additionalContext
#      sentinel that proves the hook fired even if rewrite is unsupported.
#
# After running a /skill in a session with this hook registered, ask the model:
#   "Repeat the slash-command body you just received, verbatim."
#
# Interpretation:
#   - Model echoes SQUEEZ_REWRITE_SENTINEL  → rewrite IS supported → build Layer 3.
#   - Model echoes the ORIGINAL skill body, but mentions SQUEEZ_FIRED_SENTINEL
#       → hook fired but rewrite NOT supported → Layer 3 not viable; file a CC
#         feature request and rely on Layers 1+2.
#   - Neither sentinel appears → hook did not fire for this expansion type.

SPIKE_DIR="$(cd "$(dirname "$0")" && pwd)"
input=$(cat)
printf '%s\n' "$input" > "$SPIKE_DIR/expansion-input.json"

# Candidate rewrite fields. We deliberately emit several shapes; Claude Code
# ignores keys it doesn't recognise, so the one that "wins" tells us the API.
cat <<'JSON'
{
  "updatedPrompt": "SQUEEZ_REWRITE_SENTINEL (updatedPrompt field)",
  "hookSpecificOutput": {
    "hookEventName": "UserPromptExpansion",
    "updatedExpansion": "SQUEEZ_REWRITE_SENTINEL (hookSpecificOutput.updatedExpansion)",
    "updatedPrompt": "SQUEEZ_REWRITE_SENTINEL (hookSpecificOutput.updatedPrompt)",
    "additionalContext": "SQUEEZ_FIRED_SENTINEL — UserPromptExpansion hook executed."
  }
}
JSON
