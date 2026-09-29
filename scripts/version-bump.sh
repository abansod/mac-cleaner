#!/usr/bin/env bash
# Bump the package version from Commitizen conventional commits on a pull request.
#
# Commitizen bump map (conventional commits):
#   type!:  or a BREAKING CHANGE footer   -> major
#   feat                                   -> minor
#   fix, refactor, perf                    -> patch
#   chore, docs, style, test, build, ci, revert -> no bump
#
# The highest bump in <base-ref>..HEAD wins. The bump is applied once, from the
# version at the merge base, so re-running on a branch that already contains
# chore(release): bump version to X does not bump again.
#
# Usage:
#   scripts/version-bump.sh <base-ref>
#   scripts/version-bump.sh --self-test
set -euo pipefail

message_level() {
  local msg="$1"
  local subject subject_lc msg_lc
  subject=$(printf '%s\n' "$msg" | head -n 1)
  case "$subject" in
    Merge\ *)
      printf '%s\n' none
      return
      ;;
  esac

  subject_lc=$(printf '%s' "$subject" | tr '[:upper:]' '[:lower:]')
  msg_lc=$(printf '%s\n' "$msg" | tr '[:upper:]' '[:lower:]')

  if printf '%s\n' "$subject_lc" | grep -Eq '^[a-z]+(\([^)]+\))?!:'; then
    printf '%s\n' major
    return
  fi
  if printf '%s\n' "$msg_lc" | grep -Eq '^breaking[- ]change:'; then
    printf '%s\n' major
    return
  fi
  if printf '%s\n' "$subject_lc" | grep -Eq '^feat(\([^)]+\))?:'; then
    printf '%s\n' minor
    return
  fi
  if printf '%s\n' "$subject_lc" | grep -Eq '^(fix|refactor|perf)(\([^)]+\))?:'; then
    printf '%s\n' patch
    return
  fi
  printf '%s\n' none
}

level_rank() {
  case "$1" in
    major) printf '%s\n' 3 ;;
    minor) printf '%s\n' 2 ;;
    patch) printf '%s\n' 1 ;;
    *) printf '%s\n' 0 ;;
  esac
}

higher_level() {
  local left="$1"
  local right="$2"
  local left_rank right_rank
  left_rank=$(level_rank "$left")
  right_rank=$(level_rank "$right")
  if [[ "$right_rank" -gt "$left_rank" ]]; then
    printf '%s\n' "$right"
  else
    printf '%s\n' "$left"
  fi
}

apply_bump() {
  local version="$1"
  local level="$2"
  local major minor patch
  IFS=. read -r major minor patch <<EOF
$version
EOF
  case "$level" in
    major) printf '%s\n' "$((major + 1)).0.0" ;;
    minor) printf '%s\n' "${major}.$((minor + 1)).0" ;;
    patch) printf '%s\n' "${major}.${minor}.$((patch + 1))" ;;
    *) printf '%s\n' "$version" ;;
  esac
}

valid_version() {
  printf '%s\n' "$1" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
}

version_cmp() {
  local left="$1"
  local right="$2"
  local l1 l2 l3 r1 r2 r3
  IFS=. read -r l1 l2 l3 <<EOF
$left
EOF
  IFS=. read -r r1 r2 r3 <<EOF
$right
EOF
  if [[ "$l1" -ne "$r1" ]]; then
    if [[ "$l1" -lt "$r1" ]]; then printf '%s\n' -1; else printf '%s\n' 1; fi
    return
  fi
  if [[ "$l2" -ne "$r2" ]]; then
    if [[ "$l2" -lt "$r2" ]]; then printf '%s\n' -1; else printf '%s\n' 1; fi
    return
  fi
  if [[ "$l3" -ne "$r3" ]]; then
    if [[ "$l3" -lt "$r3" ]]; then printf '%s\n' -1; else printf '%s\n' 1; fi
    return
  fi
  printf '%s\n' 0
}

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

write_package_version() {
  local file="$1"
  local version="$2"
  local tmp
  tmp="$(mktemp)"
  awk -v ver="$version" '
    BEGIN { done = 0 }
    /^version = "/ && !done {
      print "version = \"" ver "\""
      done = 1
      next
    }
    { print }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

write_lock_version() {
  local file="$1"
  local version="$2"
  local tmp
  tmp="$(mktemp)"
  awk -v ver="$version" '
    $0 == "name = \"mac-cleaner\"" { found = 1; print; next }
    found && /^version = "/ {
      print "version = \"" ver "\""
      found = 0
      next
    }
    { print }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

highest_level_in_range() {
  local range="$1"
  local level="none"
  local msg next
  while IFS= read -r -d '' msg; do
    [[ -z "$msg" ]] && continue
    next=$(message_level "$msg")
    level=$(higher_level "$level" "$next")
  done < <(git log --reverse --format='%B%x00' "$range")
  printf '%s\n' "$level"
}

bump_repo() {
  local base_ref="$1"
  local merge_base range level base_version head_version target cmp
  merge_base=$(git merge-base HEAD "$base_ref")
  range="${merge_base}..HEAD"
  level=$(highest_level_in_range "$range")
  echo "Bump level from ${range}: ${level}"
  if [[ "$level" == "none" ]]; then
    echo "No Commitizen bump in this range."
    return 0
  fi

  base_version=$(git show "${merge_base}:Cargo.toml" | awk '
    /^version = "/ {
      gsub(/^version = "/, "", $0)
      gsub(/".*$/, "", $0)
      print
      exit
    }
  ')
  if ! valid_version "$base_version"; then
    echo "Base Cargo.toml version is not x.y.z: ${base_version}" >&2
    return 1
  fi
  head_version=$(read_package_version Cargo.toml)
  if ! valid_version "$head_version"; then
    echo "HEAD Cargo.toml version is not x.y.z: ${head_version}" >&2
    return 1
  fi

  target=$(apply_bump "$base_version" "$level")
  cmp=$(version_cmp "$head_version" "$target")
  echo "Base ${base_version}, HEAD ${head_version}, target ${target}"
  if [[ "$cmp" -ge 0 ]]; then
    echo "HEAD already at or above ${target}."
    return 0
  fi

  write_package_version Cargo.toml "$target"
  if [[ -f Cargo.lock ]]; then
    write_lock_version Cargo.lock "$target"
  fi
  echo "Bumped version to ${target}"
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

  assert_eq "$(message_level "feat: add disk cleanup")" minor "feat"
  assert_eq "$(message_level "feat(scan): hash faster")" minor "feat scope"
  assert_eq "$(message_level "fix: close fd")" patch "fix"
  assert_eq "$(message_level "refactor(ui): split model")" patch "refactor"
  assert_eq "$(message_level "perf: skip stat")" patch "perf"
  assert_eq "$(message_level "docs: rewrite readme")" none "docs"
  assert_eq "$(message_level "chore(release): bump version to 0.1.0")" none "chore release"
  assert_eq "$(message_level "ci: run bump on pull requests")" none "ci"
  assert_eq "$(message_level "feat!: drop config file")" major "feat bang"
  assert_eq "$(message_level "$(printf '%s\n' 'fix: handle empty' '' 'BREAKING CHANGE: config path moved')")" major "breaking footer"
  assert_eq "$(higher_level patch minor)" minor "higher minor"
  assert_eq "$(higher_level minor major)" major "higher major"
  assert_eq "$(higher_level minor none)" minor "higher ignores none"
  assert_eq "$(apply_bump 0.0.5 patch)" 0.0.6 "patch bump"
  assert_eq "$(apply_bump 0.0.5 minor)" 0.1.0 "minor bump"
  assert_eq "$(apply_bump 0.0.5 major)" 1.0.0 "major bump"
  assert_eq "$(apply_bump 1.2.3 minor)" 1.3.0 "minor resets patch"
  assert_eq "$(version_cmp 0.0.5 0.0.6)" -1 "cmp less"
  assert_eq "$(version_cmp 0.1.0 0.1.0)" 0 "cmp equal"
  assert_eq "$(version_cmp 1.0.0 0.9.9)" 1 "cmp greater"

  local dir script
  script=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
  dir=$(mktemp -d)
  (
    cd "$dir"
    git init -q
    git config user.email "test@example.com"
    git config user.name "test"
    printf '%s\n' '[package]' 'name = "mac-cleaner"' 'version = "0.0.5"' > Cargo.toml
    printf '%s\n' '[[package]]' 'name = "mac-cleaner"' 'version = "0.0.5"' > Cargo.lock
    git add Cargo.toml Cargo.lock
    git commit -qm "chore: init"
    git commit --allow-empty -qm "feat(scan): walk faster"
    "$script" HEAD~1
    head_ver=$(awk '/^version = / { gsub(/"/, "", $3); print $3; exit }' Cargo.toml)
    lock_ver=$(awk '$0 == "name = \"mac-cleaner\"" { found = 1; next } found && /^version = / { gsub(/"/, "", $3); print $3; exit }' Cargo.lock)
    [[ "$head_ver" == "0.1.0" && "$lock_ver" == "0.1.0" ]]
    "$script" HEAD~1
    again=$(awk '/^version = / { gsub(/"/, "", $3); print $3; exit }' Cargo.toml)
    [[ "$again" == "0.1.0" ]]
  ) || failed=1
  if [[ "$failed" -eq 0 ]]; then
    echo "ok apply feat bump once"
  else
    echo "FAIL apply feat bump" >&2
  fi
  rm -rf "$dir"

  if [[ "$failed" -ne 0 ]]; then
    echo "version-bump self-test failed" >&2
    return 1
  fi
  echo "version-bump self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit 0
fi

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <base-ref>|--self-test" >&2
  exit 1
fi

bump_repo "$1"
