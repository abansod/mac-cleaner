#!/usr/bin/env bash
# Create the GitHub release tag for the version in Cargo.toml.
#
# The pull request workflow writes that version. After the pull request
# merges, this script tags v<version> when the tag does not already exist.
#
# Usage:
#   scripts/create-release-tag.sh
#   scripts/create-release-tag.sh --self-test
set -euo pipefail

read_package_version() {
  local file="$1"
  awk '
    /^version = "/ {
      gsub(/^version = "/, "", $0)
      gsub(/".*$/, "", $0)
      print
      exit
    }
  ' "$file"
}

valid_version() {
  printf '%s\n' "$1" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
}

tag_for_version() {
  printf 'v%s\n' "$1"
}

tag_exists() {
  local tag="$1"
  git ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1
}

create_release_tag() {
  local version tag target
  version=$(read_package_version Cargo.toml)
  if ! valid_version "$version"; then
    echo "Cargo.toml version is not x.y.z: ${version}" >&2
    return 1
  fi
  tag=$(tag_for_version "$version")
  echo "Cargo.toml ${version} -> ${tag}"
  if tag_exists "$tag"; then
    echo "Tag ${tag} already exists."
    return 0
  fi

  target="${GITHUB_SHA:-HEAD}"
  gh release create "$tag" \
    --target "$target" \
    --title "$tag" \
    --generate-notes
  echo "Created ${tag}"
}

self_test() {
  local failed=0
  assert_eq() {
    local got="$1"
    local expected="$2"
    local label="$3"
    if [[ "$got" != "$expected" ]]; then
      echo "FAIL ${label}: got '${got}' expected '${expected}'" >&2
      failed=1
    else
      echo "ok ${label}"
    fi
  }

  assert_eq "$(tag_for_version 0.0.8)" "v0.0.8" "tag name"
  assert_eq "$(tag_for_version 1.2.3)" "v1.2.3" "tag name major"
  if valid_version "0.0.8"; then
    echo "ok valid version"
  else
    echo "FAIL valid version" >&2
    failed=1
  fi
  if valid_version ".0.0.5"; then
    echo "FAIL rejected bad version" >&2
    failed=1
  else
    echo "ok rejected bad version"
  fi

  local dir
  dir=$(mktemp -d)
  (
    cd "$dir"
    printf '%s\n' '[package]' 'name = "mac-cleaner"' 'version = "0.0.9"' > Cargo.toml
    version=$(read_package_version Cargo.toml)
    [[ "$version" == "0.0.9" ]]
    [[ "$(tag_for_version "$version")" == "v0.0.9" ]]
  ) || failed=1
  if [[ "$failed" -eq 0 ]]; then
    echo "ok read Cargo.toml version"
  else
    echo "FAIL read Cargo.toml version" >&2
  fi
  rm -rf "$dir"

  if [[ "$failed" -ne 0 ]]; then
    echo "create-release-tag self-test failed" >&2
    return 1
  fi
  echo "create-release-tag self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit 0
fi

if [[ $# -ne 0 ]]; then
  echo "usage: $0 [--self-test]" >&2
  exit 1
fi

create_release_tag
