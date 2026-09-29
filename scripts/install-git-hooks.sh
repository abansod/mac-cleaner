#!/usr/bin/env bash
# Point this clone at the Commitizen commit-msg hook in .githooks/.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$root/.git/hooks"
ln -sfn ../../.githooks/commit-msg "$root/.git/hooks/commit-msg"
echo "Installed .git/hooks/commit-msg -> .githooks/commit-msg"
