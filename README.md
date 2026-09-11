# Mac Cleaner — CleanMyMac-style CLI for macOS

Free disk space by finding and removing:

- **System & user caches**
- **Logs** and temporary files
- **Trash**
- **Old installers** in Downloads (DMG/PKG/ZIP)
- **Browser caches** (Safari, Chrome, Firefox, Edge, Brave, Arc)
- **Xcode junk** (DerivedData, simulators, SwiftPM, CocoaPods)
- **Mail downloads**
- **App leftovers** (Application Support / saved state for missing apps)
- **Large & old files**
- **Duplicate files** (content-hashed)
- **Unused language files** in `~/Applications`

You always review results first. Move with the arrow keys, mark files, and confirm before anything is deleted.

## Requirements

- macOS
- A terminal (for the interactive UI)
- [Rust](https://rustup.rs/) only if you build from source — Homebrew users do not need it

## Install

### Homebrew (recommended)

This repo doubles as a Homebrew tap (`Formula/mac-cleaner.rb`). Stable installs download a prebuilt universal macOS binary from GitHub Releases (no Rust or Cargo). `brew install --HEAD` still compiles from source and needs Rust.

After a GitHub Release, install with:

```bash
brew tap abansod/mac-cleaner https://github.com/abansod/mac-cleaner
brew install mac-cleaner
```

Upgrade later with:

```bash
brew update
brew upgrade mac-cleaner
```

> Publishing: create a GitHub Release tagged `vX.Y.Z` (crate version in `Cargo.toml` must match). The [Release & Homebrew tap](.github/workflows/release-brew.yml) workflow builds a universal macOS binary, attaches `mac-cleaner-vX.Y.Z-macos.tar.gz` to the release, and bumps the formula `url`/`sha256`/`version` on `main` (or on an external tap if configured).

Optional: to publish the formula to a **separate** tap instead of this repo, set repository variable `HOMEBREW_TAP` (e.g. `abansod/homebrew-tap`) and secret `HOMEBREW_TAP_TOKEN` (PAT with `repo` scope on that tap).

### From source

```bash
git clone https://github.com/abansod/mac-cleaner.git
cd mac-cleaner
cargo install --path .
mac-cleaner
```

Or run without installing:

```bash
cargo run --release
```

## Usage

Interactive full scan (default):

```bash
mac-cleaner
```

Faster junk-only scan (skips duplicates, leftovers, large files, languages):

```bash
mac-cleaner --smart
```

Duplicates only:

```bash
mac-cleaner duplicates
mac-cleaner duplicates --path ~/Pictures
```

Print findings without the UI:

```bash
mac-cleaner list
mac-cleaner list --smart
mac-cleaner scan --mode full --list
```

Non-interactive clean (confirm required unless `-y`):

```bash
mac-cleaner clean --category "User Caches"
mac-cleaner clean --smart -y   # careful
```

### Interactive controls

| Key | Action |
|-----|--------|
| `↑` `↓` / `j` `k` | Move highlight |
| `Enter` | Open category or group · delete marked (or highlighted) files |
| `Space` | Mark / unmark a file |
| `K` | Keep the highlighted file, delete the rest (duplicates) |
| `d` | Delete the current group |
| `a` | Delete every group in this category |
| `g` / `G` | Jump to first / last |
| `r` | Scan again |
| `Esc` / `b` | Back |
| `?` | Help |
| `q` | Quit |

Click a row to highlight it. A confirm dialog appears before every delete (`←` `→` or `y`/`n`).

**Duplicates:** open a set, highlight the copy to keep, press `K`.

## Safety

- Only deletes paths under your home directory (plus writable `/tmp` and some `/Library/Caches`).
- Refuses protected system prefixes (`/System`, `/usr`, …).
- Asks for confirmation before every delete (unless `--yes`).
- Duplicate “delete group” removes **all** copies — prefer `K` to keep one.

## Development

```bash
cargo build
cargo test
cargo run -- --version
cargo fmt
cargo clippy --all-targets -- -D warnings
```

`Cargo.lock` is the source of truth for reproducible builds.

## License

This project is licensed under the [MIT License](LICENSE).
