#!/usr/bin/env bash
# Time read-only scans of this Mac with hyperfine. Nothing is deleted.
# Results depend on disk contents and the filesystem cache; compare runs on one machine.
# Extra arguments go to hyperfine, e.g. --runs 10 or --prepare 'sudo purge' for cold-cache runs.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "hyperfine not found; install it with: brew install hyperfine" >&2
  exit 1
fi

cargo build --release
BIN="target/release/mac-cleaner"

hyperfine --warmup 1 --runs 5 \
  --export-markdown target/bench-scan.md \
  "$@" \
  "${BIN} list --mode smart" \
  "${BIN} list"

echo "results written to target/bench-scan.md"
