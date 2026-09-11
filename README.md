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

You always review results first. Delete an **entire group**, **multiple files**, or keep a few and delete the rest.

## Requirements

- macOS
- Python 3.10+
- [uv](https://docs.astral.sh/uv/) (recommended) or pip + venv

## Install

### With uv (recommended)

```bash
git clone https://github.com/akshaybansod/mac-cleaner.git
cd mac-cleaner
uv sync
uv run mac-cleaner
```

### With pip + venv

```bash
git clone https://github.com/akshaybansod/mac-cleaner.git
cd mac-cleaner
python3 -m venv .venv
source .venv/bin/activate
pip install -e .
mac-cleaner
```

## Usage

Interactive full scan (default):

```bash
uv run mac-cleaner
# or, with an activated venv:
mac-cleaner
python -m mac_cleaner
```

Smart scan (faster junk-only subset):

```bash
uv run mac-cleaner scan --mode smart
```

Duplicates only:

```bash
uv run mac-cleaner duplicates
uv run mac-cleaner duplicates --path ~/Pictures
```

List results without interactive delete:

```bash
uv run mac-cleaner scan --mode full --list
```

Non-interactive clean (confirm required unless `-y`):

```bash
uv run mac-cleaner clean --category "User Caches"
uv run mac-cleaner clean --smart -y   # careful
```

### Interactive controls

| Key | Action |
|-----|--------|
| `1–N` | Open category / group |
| `1,3,5-7` | Delete selected files (comma + ranges) |
| `k 1` / `k 1,3` | **Keep** these files, delete the rest |
| `s` | Mark mode — toggle files, then `d` to delete |
| `d` | Delete entire group (or marked files) |
| `a` | Delete all groups in category |
| `n` / `p` | Next / previous page |
| `b` | Back |
| `r` | Rescan |
| `h` | Help |
| `q` | Quit |

**Keep example (duplicates):** open a duplicate set, then type `k 1` to keep the first copy and remove the others.

## Safety

- Only deletes paths under your home directory (plus writable `/tmp` and some `/Library/Caches`).
- Refuses protected system prefixes (`/System`, `/usr`, …).
- Asks for confirmation before every delete (unless `--yes`).
- Duplicate “delete group” removes **all** copies — prefer `k 1` or deleting individual extras.

## Development

```bash
uv sync
uv run mac-cleaner --version
uv lock                    # refresh uv.lock after dependency changes
uv export --no-dev --no-hashes -o requirements.txt   # for pip users
```

`uv.lock` is the source of truth for reproducible installs. `requirements.txt` is exported for classic pip workflows.

## License

This project is licensed under the [MIT License](LICENSE).
