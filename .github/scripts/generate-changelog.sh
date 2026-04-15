#!/usr/bin/env bash
# generate-changelog.sh — Prepend a new version section to CHANGELOG.md
# from git log between tags. Called by release.yml after publish.
#
# Environment:
#   NEW_TAG   — the tag being released (e.g. v0.3.0)
#
# Derives PREV_TAG automatically from git tags.
set -euo pipefail

NEW_TAG="${NEW_TAG:?NEW_TAG is required}"
VERSION="${NEW_TAG#v}"
DATE=$(git log -1 --format=%ai "$NEW_TAG" 2>/dev/null | cut -d' ' -f1)
[ -z "$DATE" ] && DATE=$(date +%Y-%m-%d)

# Find previous tag
PREV_TAG=$(git tag --sort=-v:refname | grep -v "^${NEW_TAG}$" | head -1)
if [ -z "$PREV_TAG" ]; then
  echo "No previous tag found — using full history"
  COMMITS=$(git log "$NEW_TAG" --oneline --no-merges)
else
  COMMITS=$(git log "${PREV_TAG}..${NEW_TAG}" --oneline --no-merges)
fi

# Categorize commits
ADDED=""
CHANGED=""
FIXED=""

while IFS= read -r line; do
  [ -z "$line" ] && continue
  # Strip hash prefix
  MSG="${line#* }"

  # Categorize by conventional commit prefix
  case "$MSG" in
    feat:*|feat\(*) ADDED="${ADDED}\n- ${MSG#feat: }" ;;
    fix:*|fix\(*)   FIXED="${FIXED}\n- ${MSG#fix: }" ;;
    *)               CHANGED="${CHANGED}\n- ${MSG}" ;;
  esac
done <<< "$COMMITS"

# Build the new section
SECTION="## [${VERSION}] - ${DATE}"
[ -n "$ADDED" ]   && SECTION="${SECTION}\n\n### Added${ADDED}"
[ -n "$CHANGED" ]  && SECTION="${SECTION}\n\n### Changed${CHANGED}"
[ -n "$FIXED" ]    && SECTION="${SECTION}\n\n### Fixed${FIXED}"

# Add compare link
if [ -n "$PREV_TAG" ]; then
  LINK="[${VERSION}]: https://github.com/claudioemmanuel/squeez/compare/${PREV_TAG}...${NEW_TAG}"
else
  LINK="[${VERSION}]: https://github.com/claudioemmanuel/squeez/releases/tag/${NEW_TAG}"
fi

# Prepend to CHANGELOG.md after the [Unreleased] section
if [ ! -f CHANGELOG.md ]; then
  echo "CHANGELOG.md not found — creating"
  printf "# Changelog\n\n## [Unreleased]\n\n" > CHANGELOG.md
fi

# Use python3 for reliable multi-line insertion
python3 - "$SECTION" "$LINK" "$NEW_TAG" <<'PYEOF'
import sys, re

section = sys.argv[1]
link = sys.argv[2]
new_tag = sys.argv[3]
version = new_tag.lstrip('v')

with open("CHANGELOG.md", "r") as f:
    content = f.read()

# Insert new section after ## [Unreleased] line
marker = "## [Unreleased]"
if marker in content:
    content = content.replace(
        marker,
        marker + "\n\n" + section.replace("\\n", "\n"),
        1
    )
else:
    # Fallback: insert after first heading
    content = re.sub(
        r"(# Changelog.*?\n)",
        r"\1\n" + section.replace("\\n", "\n") + "\n",
        content,
        count=1
    )

# Update [Unreleased] compare link
unreleased_pattern = r"\[Unreleased\]: https://github\.com/claudioemmanuel/squeez/compare/v[\d.]+\.\.\.HEAD"
unreleased_link = f"[Unreleased]: https://github.com/claudioemmanuel/squeez/compare/{new_tag}...HEAD"
if re.search(unreleased_pattern, content):
    content = re.sub(unreleased_pattern, unreleased_link, content)

# Add version compare link if not present
if f"[{version}]:" not in content:
    content = content.rstrip() + "\n" + link + "\n"

with open("CHANGELOG.md", "w") as f:
    f.write(content)

print(f"CHANGELOG.md updated with {new_tag}")
PYEOF
