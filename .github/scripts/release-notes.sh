#!/usr/bin/env bash
# release-notes.sh — Emit GitHub Release notes for NEW_TAG to stdout.
#
# Built from git history (categorized conventional commits between the previous
# tag and NEW_TAG), NOT from CHANGELOG.md. The CHANGELOG entry for a release is
# generated *after* the GitHub Release is published (release.yml → update-docs),
# so reading CHANGELOG.md here would always miss the current version. Generating
# straight from git guarantees the release body always describes what changed.
#
# Environment:
#   NEW_TAG — the tag being released (e.g. v1.33.0)
#
# Requires full history + tags (checkout with fetch-depth: 0).
set -euo pipefail

NEW_TAG="${NEW_TAG:?NEW_TAG is required}"
VERSION="${NEW_TAG#v}"

# Find previous tag (highest version tag that isn't NEW_TAG).
PREV_TAG=$(git tag --sort=-v:refname | grep -v "^${NEW_TAG}$" | head -1 || true)
if [ -z "$PREV_TAG" ]; then
  COMMITS=$(git log "$NEW_TAG" --oneline --no-merges)
else
  COMMITS=$(git log "${PREV_TAG}..${NEW_TAG}" --oneline --no-merges)
fi

# Categorize commits by conventional-commit prefix.
ADDED=""
CHANGED=""
FIXED=""
while IFS= read -r line; do
  [ -z "$line" ] && continue
  MSG="${line#* }"  # strip the short hash
  # Drop automated release-plumbing commits — they describe the release
  # machinery, not what changed for users.
  case "$MSG" in
    "chore(release):"*|"docs: update changelog"*) continue ;;
  esac
  case "$MSG" in
    feat:*|feat\(*) ADDED="${ADDED}\n- ${MSG#feat: }" ;;
    fix:*|fix\(*)   FIXED="${FIXED}\n- ${MSG#fix: }" ;;
    *)              CHANGED="${CHANGED}\n- ${MSG}" ;;
  esac
done <<< "$COMMITS"

# Build the body.
BODY=""
[ -n "$ADDED" ]   && BODY="${BODY}### Added${ADDED}\n\n"
[ -n "$CHANGED" ] && BODY="${BODY}### Changed${CHANGED}\n\n"
[ -n "$FIXED" ]   && BODY="${BODY}### Fixed${FIXED}\n\n"

# Fallback: never emit an empty body.
if [ -z "$BODY" ]; then
  BODY="Release ${VERSION}.\n\n"
fi

# Compare / tag link.
if [ -n "$PREV_TAG" ]; then
  BODY="${BODY}**Full changelog**: https://github.com/claudioemmanuel/squeez/compare/${PREV_TAG}...${NEW_TAG}\n"
else
  BODY="${BODY}https://github.com/claudioemmanuel/squeez/releases/tag/${NEW_TAG}\n"
fi

printf '%b' "$BODY"
