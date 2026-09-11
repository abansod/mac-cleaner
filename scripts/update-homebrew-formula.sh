#!/usr/bin/env bash
# Update Formula/mac-cleaner.rb for a given git tag (e.g. v2.0.0).
# Points url at the universal macOS tarball on GitHub Releases.
set -euo pipefail

TAG="${1:?usage: $0 <tag> [repo] [formula]}"
REPO="${2:-abansod/mac-cleaner}"
VERSION="${TAG#v}"
URL="https://github.com/${REPO}/releases/download/${TAG}/mac-cleaner-${TAG}-macos.tar.gz"
FORMULA_PATH="${3:-Formula/mac-cleaner.rb}"

echo "Fetching ${URL}"
SHA256="$(curl -fsSL "${URL}" | shasum -a 256 | awk '{print $1}')"
echo "sha256: ${SHA256}"

tmp="$(mktemp)"
# Rewrite stable url (source archive or prior release asset), sha256, and version.
# Leave homepage and the head git url alone.
sed -E \
  -e "s|url \"https://github.com/[^\"]+/archive/refs/tags/v[^\"]+\\.tar\\.gz\"|url \"${URL}\"|" \
  -e "s|url \"https://github.com/[^\"]+/releases/download/v[^\"]+/mac-cleaner-v[^\"]+-macos\\.tar\\.gz\"|url \"${URL}\"|" \
  -e "s|sha256 \"[0-9a-fA-F]{64}\"|sha256 \"${SHA256}\"|" \
  -e "s|^([[:space:]]*)version \"[^\"]+\"|\\1version \"${VERSION}\"|" \
  "${FORMULA_PATH}" > "${tmp}"

if ! grep -qE '^[[:space:]]*version "' "${tmp}"; then
  inserted="$(mktemp)"
  sed -E "s|^([[:space:]]*homepage \".*\")$|\\1\\
  version \"${VERSION}\"|" "${tmp}" > "${inserted}"
  mv "${inserted}" "${tmp}"
fi

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
if ! grep -qE "^[[:space:]]*version \"${VERSION}\"" "${tmp}"; then
  echo "failed to patch version in ${FORMULA_PATH}" >&2
  rm -f "${tmp}"
  exit 1
fi

mv "${tmp}" "${FORMULA_PATH}"
echo "updated ${FORMULA_PATH} → v${VERSION}"
