# Contributing

Bug reports and pull requests are welcome on [GitHub](https://github.com/abansod/mac-cleaner).

## Before you open a pull request

Run the checks in the [Development](README.md#development) section:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Commit messages

Use [Commitizen](https://commitizen-tools.github.io/commitizen/) conventional commits. The pull request workflow reads these messages and writes the new package version into `Cargo.toml` and `Cargo.lock` on the pull request branch.

```text
type(scope): description

Optional body, separated from the subject by a blank line.
```

`scope` is optional. Mark a breaking change with `!` before the colon (`feat!:` or `feat(scan)!:`), or with a `BREAKING CHANGE:` footer.

| Message | Version bump |
| --- | --- |
| `feat:` | minor (`0.0.5` → `0.1.0`) |
| `fix:`, `refactor:`, `perf:` | patch (`0.0.5` → `0.0.6`) |
| `type!:` or a `BREAKING CHANGE:` footer | major (`0.0.5` → `1.0.0`) |
| `chore:`, `docs:`, `style:`, `test:`, `build:`, `ci:`, `revert:` | none |

Allowed types: `feat`, `fix`, `refactor`, `perf`, `chore`, `docs`, `style`, `test`, `build`, `ci`, `revert`.

The highest bump in the pull request wins. CI records it as `chore(release): bump version to …`, and that commit is ignored the next time the check runs.

Keep the subject within 72 characters when you can. The hook rejects a subject longer than 100 characters.

### Commit-msg hook

Git checks the message with the `commit-msg` hook, after you write it and before the commit is created. Install it once per clone:

```bash
./scripts/install-git-hooks.sh
```

That links `.git/hooks/commit-msg` to `.githooks/commit-msg`. `Merge …`, `fixup!`, and `squash!` subjects are left alone, as is git's default `Revert "…"` subject.

CI runs the same check on every pull request (`scripts/check-commit-msg.sh`).

## Release

`Cargo.toml` is the version source.

1. The pull request workflow bumps `Cargo.toml` and `Cargo.lock` from the Commitizen messages above and commits that on the pull request branch.
2. After that pull request merges, CI tests `main`. When those tests pass, it reads `Cargo.toml` and creates GitHub release `v<version>` when that tag does not already exist. A failed test does not create a tag.
3. Publishing that release builds the universal macOS binary, attaches it to the release, and merges an update to `Formula/mac-cleaner.rb` so Homebrew installs that build.

The package version is `0.0.9`. Merging this pull request creates tag `v0.0.9` after tests pass, then publishes the formula.
