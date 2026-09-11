#!/usr/bin/env bash
# Update Formula/mac-cleaner.rb for a given git tag (e.g. v1.0.0).
set -euo pipefail

TAG="${1:?usage: $0 <tag> [repo]}"
REPO="${2:-abansod/mac-cleaner}"
VERSION="${TAG#v}"
URL="https://github.com/${REPO}/archive/refs/tags/${TAG}.tar.gz"
FORMULA_PATH="${3:-Formula/mac-cleaner.rb}"

echo "Fetching ${URL}"
SHA256="$(curl -fsSL "${URL}" | shasum -a 256 | awk '{print $1}')"
echo "sha256: ${SHA256}"

python3 - "${FORMULA_PATH}" "${VERSION}" "${URL}" "${SHA256}" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
version, url, sha256 = sys.argv[2], sys.argv[3], sys.argv[4]
text = path.read_text()
text, n_url = re.subn(
    r'url\s+"https://github\.com/[^"]+/archive/refs/tags/v[^"]+\.tar\.gz"',
    f'url "{url}"',
    text,
    count=1,
)
text, n_sha = re.subn(
    r'sha256\s+"[0-9a-fA-F]{64}"',
    f'sha256 "{sha256}"',
    text,
    count=1,
)
if n_url != 1 or n_sha != 1:
    raise SystemExit(f"failed to patch formula (url={n_url}, sha256={n_sha})")
# Keep version in sync when an explicit version stanza exists
text = re.sub(
    r'^(\s*)version\s+".*"\s*$',
    rf'\1version "{version}"',
    text,
    count=1,
    flags=re.M,
)
path.write_text(text)
print(f"updated {path} → v{version}")
PY
