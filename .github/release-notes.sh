#!/usr/bin/env bash
# Prints the section of CHANGELOG.md for one version: everything between "## <version>" and the next "## ".
# Fails when there is no such section, or it says nothing.
set -euo pipefail
version="${1:?usage: release-notes.sh <version>}"
notes="$(awk -v heading="## $version" '
  /^## / { if (found) exit; if ($0 == heading) { found = 1; next } }
  found { print }
' CHANGELOG.md)"
if ! printf '%s' "$notes" | grep -q '[^[:space:]]'; then
  echo "CHANGELOG.md has no section \"## $version\" with notes in it. Add one before releasing." >&2
  exit 1
fi
printf '%s\n' "$notes"
