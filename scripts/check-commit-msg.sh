#!/usr/bin/env bash
# Check that a commit message follows Commitizen conventional commits.
#
#   type(scope): description
#
#   Optional body, after a blank line.
#
# Types: build, ci, docs, feat, fix, perf, refactor, style, test, chore, revert
# scope is optional. "!" before ":" marks a breaking change (feat!: or feat(scope)!:).
#
# Merge, fixup!, squash!, and git's default Revert "..." subjects are allowed.
#
# Usage:
#   scripts/check-commit-msg.sh <message-file>
#   scripts/check-commit-msg.sh --range <git-rev-range>
#   scripts/check-commit-msg.sh --self-test
set -euo pipefail

TYPES='build|ci|docs|feat|fix|perf|refactor|style|test|chore|revert'
SUBJECT_MAX=100

usage() {
  echo "usage: $0 <message-file>|--range <rev-range>|--self-test" >&2
}

trim_blank_lines() {
  awk '
    {
      sub(/\r$/, "")
      sub(/[[:space:]]+$/, "")
      lines[NR] = $0
    }
    END {
      if (NR == 0) exit
      start = 1
      end = NR
      while (start <= end && lines[start] ~ /^$/) start++
      while (end >= start && lines[end] ~ /^$/) end--
      for (i = start; i <= end; i++) print lines[i]
    }
  '
}

prepare_message() {
  local raw="$1"
  printf '%s\n' "$raw" | grep -v '^#' | trim_blank_lines || true
}

is_exempt() {
  local subject="$1"
  printf '%s\n' "$subject" | grep -Eq '^(Merge .+|fixup! .+|squash! .+|Revert ".+")$'
}

subject_pattern() {
  printf '%s\n' "^(${TYPES})(\\([A-Za-z0-9._/-]+\\))?!?: [^[:space:]].*$"
}

reject() {
  local subject="$1"
  shift
  echo "Commitizen commit message rejected." >&2
  echo "  $*" >&2
  echo "  got: ${subject}" >&2
  echo "Use: type(scope): description" >&2
  echo "Types: build, ci, docs, feat, fix, perf, refactor, style, test, chore, revert" >&2
  echo "scope is optional. Put ! before : for a breaking change." >&2
  return 1
}

check_text() {
  local raw="$1"
  local clean subject second length pattern
  clean=$(prepare_message "$raw")
  if [[ -z "$clean" ]]; then
    echo "Commitizen commit message rejected: message is empty." >&2
    return 1
  fi

  subject=$(printf '%s\n' "$clean" | head -n 1)
  if is_exempt "$subject"; then
    return 0
  fi

  second=$(printf '%s\n' "$clean" | sed -n '2p')
  if [[ -n "$second" ]]; then
    reject "$subject" "Leave a blank line between the subject and the body."
    return 1
  fi

  pattern=$(subject_pattern)
  if ! printf '%s\n' "$subject" | grep -Eq "$pattern"; then
    reject "$subject" "Subject must look like: type(scope): description"
    return 1
  fi

  length=$(printf '%s' "$subject" | wc -c | tr -d '[:space:]')
  if [[ "$length" -gt "$SUBJECT_MAX" ]]; then
    reject "$subject" "Subject is ${length} characters. Keep it within ${SUBJECT_MAX}."
    return 1
  fi
}

check_file() {
  local file="$1"
  if [[ ! -f "$file" ]]; then
    echo "commit message file not found: ${file}" >&2
    return 1
  fi
  check_text "$(cat "$file")"
}

check_range() {
  local range="$1"
  local sha failed=0 short
  while IFS= read -r sha; do
    [[ -z "$sha" ]] && continue
    if ! check_text "$(git log -1 --format=%B "$sha")"; then
      short=$(git rev-parse --short "$sha")
      echo "commit ${short} does not follow Commitizen format" >&2
      failed=1
    fi
  done < <(git rev-list --reverse "$range")
  return "$failed"
}

self_test() {
  local failed=0
  assert_ok() {
    local msg="$1"
    local label="$2"
    if check_text "$msg"; then
      echo "ok ${label}"
    else
      echo "FAIL ${label}: expected accept" >&2
      failed=1
    fi
  }
  assert_bad() {
    local msg="$1"
    local label="$2"
    if check_text "$msg" >/dev/null 2>&1; then
      echo "FAIL ${label}: expected reject" >&2
      failed=1
    else
      echo "ok ${label}"
    fi
  }

  assert_ok "feat: add disk cleanup" "feat"
  assert_ok "feat(scan): walk faster" "feat scope"
  assert_ok "feat!: drop the config file" "breaking bang"
  assert_ok "feat(scan)!: drop the walker" "breaking scope"
  assert_ok "fix: close the file handle" "fix"
  assert_ok "chore(release): bump version to 0.1.0" "chore release"
  assert_ok "ci: check commit messages" "ci"
  assert_ok "$(printf '%s\n' 'fix: handle an empty scan' '' 'The scanner returned no rows.')" "body"
  assert_ok "$(printf '%s\n' 'fix: handle an empty scan' '' 'BREAKING CHANGE: the scan flag moved')" "breaking footer"
  assert_ok "Merge pull request #3 from abansod/feature/example" "merge"
  assert_ok "fixup! feat: add disk cleanup" "fixup"
  assert_ok 'Revert "feat: add disk cleanup"' "git revert"
  assert_ok "$(printf '%s\n' '# comment' 'docs: rewrite the readme')" "ignores comments"

  assert_bad "Update the readme" "missing type"
  assert_bad "Feat: add disk cleanup" "uppercase type"
  assert_bad "feat:add disk cleanup" "missing space"
  assert_bad "feat:" "empty description"
  assert_bad "feat(): add disk cleanup" "empty scope"
  assert_bad "wip: still working" "unknown type"
  assert_bad "$(printf '%s\n' 'feat: add disk cleanup' 'Forgot the blank line.')" "body without blank line"
  assert_bad "feat: $(printf 'x%.0s' $(seq 1 120))" "subject too long"

  local dir script
  script=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
  dir=$(mktemp -d)
  (
    cd "$dir"
    git init -q
    git config user.email "test@example.com"
    git config user.name "test"
    git commit --allow-empty -qm "chore: init"
    git commit --allow-empty -qm "feat: add disk cleanup"
    git commit --allow-empty -qm "docs: note the hook"
    "$script" --range HEAD~2..HEAD
    git commit --allow-empty -qm "Update the readme"
    if "$script" --range HEAD~1..HEAD >/dev/null 2>&1; then
      echo "range accepted a bad commit" >&2
      exit 1
    fi
  ) || failed=1
  if [[ "$failed" -eq 0 ]]; then
    echo "ok range check"
  else
    echo "FAIL range check" >&2
  fi
  rm -rf "$dir"

  if [[ "$failed" -ne 0 ]]; then
    echo "commit message self-test failed" >&2
    return 1
  fi
  echo "commit message self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit 0
fi

if [[ "${1:-}" == "--range" ]]; then
  if [[ $# -ne 2 ]]; then
    usage
    exit 1
  fi
  check_range "$2"
  exit 0
fi

if [[ $# -ne 1 ]]; then
  usage
  exit 1
fi

check_file "$1"
