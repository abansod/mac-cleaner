#!/usr/bin/env bash
# Update Formula/mac-cleaner.rb for a given git tag (e.g. v2.0.0).
set -euo pipefail

TAG="${1:?usage: $0 <tag> [repo] [formula]}"
REPO="${2:-abansod/mac-cleaner}"
VERSION="${TAG#v}"
URL="https://github.com/${REPO}/archive/refs/tags/${TAG}.tar.gz"
FORMULA_PATH="${3:-Formula/mac-cleaner.rb}"

echo "Fetching ${URL}"
SHA256="$(curl -fsSL "${URL}" | shasum -a 256 | awk '{print $1}')"
echo "sha256: ${SHA256}"

tmp="$(mktemp)"
# Only rewrite the GitHub archive url / sha256 stanzas — leave homepage alone.
sed -E \
  -e "s|url \"https://github.com/[^\"]+/archive/refs/tags/v[^\"]+\\.tar\\.gz\"|url \"${URL}\"|" \
  -e "s|sha256 \"[0-9a-fA-F]{64}\"|sha256 \"${SHA256}\"|" \
  "${FORMULA_PATH}" > "${tmp}"

if ! grep -q "${SHA256}" "${tmp}"; then
  echo "failed to patch sha256 in ${FORMULA_PATH}" >&2
  rm -f "${tmp}"
  exit 1
fi
if ! grep -q "${URL}" "${tmp}"; then
  echo "failed to patch url in ${FORMULA_PATH}" >&2
  rm -f "${tmp}"
  exit 1
fi

mv "${tmp}" "${FORMULA_PATH}"
echo "updated ${FORMULA_PATH} → v${VERSION}"
