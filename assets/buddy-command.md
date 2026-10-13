---
description: Show the companion duck (card, stats, pet, rename)
---
The user wants to interact with the squeez companion duck. Request: $ARGUMENTS

Run the duck CLI and show the output to the user **verbatim, in a code block**
(the ASCII art depends on alignment):

- No arguments or "card": `node ~/.claude/squeez/buddy/buddy-cli.js card`
- "stats" or "status": `node ~/.claude/squeez/buddy/buddy-cli.js stats`
- "pet" / "pat": `node ~/.claude/squeez/buddy/buddy-cli.js pet`
- "rename <name>" / "call it <name>": `node ~/.claude/squeez/buddy/buddy-cli.js rename <name>`
- "reset" / "zero the XP": `node ~/.claude/squeez/buddy/buddy-cli.js reset`

After the block, at most one line of comment — the duck speaks for itself.

The main XP source is squeez token savings: 25,000 net tokens saved = 1 XP, no
cap. Actions (edits, commits, a suite that goes from red to green) are worth
little and stop at a cap of 8 XP per session — ranking up has to come from real
savings, not grinding. The art changes with every rarity color, so progress is
visible: hatchling (Common) → young (Uncommon) → adult in the water (Rare) →
wings spread (Epic) → crowned (Legendary).
