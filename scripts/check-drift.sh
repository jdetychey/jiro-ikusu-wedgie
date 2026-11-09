#!/usr/bin/env bash
# Fails if regenerated files differ from the committed ones, including new untracked
# files. Usage: check-drift.sh <path>..., relative to the repository root.
set -euo pipefail

[ "$#" -ge 1 ] || { echo "usage: check-drift.sh <path>..." >&2; exit 2; }
cd "$(dirname "${BASH_SOURCE[0]}")/.."

for path in "$@"; do
  [ -e "$path" ] || git ls-files --error-unmatch -- "$path" >/dev/null 2>&1 ||
    { echo "check-drift: $path matches no file (paths are relative to the repository root)" >&2; exit 2; }
done

git diff --exit-code -- "$@"
status="$(git status --porcelain -- "$@")"
if [ -n "$status" ]; then
  echo "$status"
  echo "check-drift: the files above are new, staged or otherwise not committed"
  exit 1
fi
