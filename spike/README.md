# Layer 0 spike — UserPromptExpansion rewrite capability

Goal: prove (or rule out) whether the `UserPromptExpansion` hook can **rewrite**
the expanded slash-command/skill body before the model sees it. This gates
Layer 3 (runtime compression of user-typed `/skill` bodies — the largest bloat
source in the band-graph transcript).

This needs a **fresh interactive Claude Code session** to observe — it cannot be
verified from inside a running turn.

## Steps

1. Make the probe executable:
   ```bash
   chmod +x spike/userpromptexpansion-spike.sh
   ```

2. Register it temporarily in `~/.claude/settings.json` under
   `hooks.UserPromptExpansion` (matcher `*` or a specific skill name):
   ```json
   {
     "hooks": {
       "UserPromptExpansion": [
         { "matcher": "*",
           "hooks": [ { "type": "command",
             "command": "bash /ABSOLUTE/PATH/squeez/spike/userpromptexpansion-spike.sh" } ] }
       ]
     }
   }
   ```

3. Start a **new** session. Invoke any skill, e.g. `/DEBUG_error-detective`.

4. Immediately ask: *"Repeat the slash-command body you just received, verbatim."*

5. Interpret:
   - Model echoes `SQUEEZ_REWRITE_SENTINEL` → **rewrite supported** → implement Layer 3
     (note which field won, shown in the sentinel text).
   - Model echoes the original skill body but a `SQUEEZ_FIRED_SENTINEL` line is
     present → hook fired, **rewrite NOT supported** → skip Layer 3, file a CC
     feature request, rely on Layers 1+2.
   - Neither sentinel → hook didn't fire for this expansion type.

6. Inspect `spike/expansion-input.json` for the exact hook input schema
   (field names like `prompt` / `command_name` / `expansion_type`).

7. **Unregister** the hook from `settings.json` when done.

## Result

Record the outcome here, then delete the `spike/` directory:

```
DATE:
WINNING FIELD (if any):
DECISION: [ build Layer 3 | skip Layer 3 + file feature request ]
```
