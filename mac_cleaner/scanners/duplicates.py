from __future__ import annotations

import hashlib
import os
from collections import defaultdict
from pathlib import Path

from mac_cleaner.models import Category, FileGroup, FileItem
from mac_cleaner.scanners.base import ProgressCallback, Scanner
from mac_cleaner.utils import HOME


class DuplicateScanner(Scanner):
    """Find duplicate files by size + content hash within common user folders."""

    name = "Duplicate Files"

    DEFAULT_ROOTS = [
        HOME / "Downloads",
        HOME / "Documents",
        HOME / "Desktop",
        HOME / "Pictures",
        HOME / "Movies",
        HOME / "Music",
    ]

    SKIP_DIRS = {
        ".git",
        ".svn",
        ".hg",
        "node_modules",
        ".Trash",
        "Library",
        ".cache",
        "__pycache__",
        ".venv",
        "venv",
        "DerivedData",
    }

    SKIP_SUFFIXES = {".ds_store", ".localized", ".sock", ".pyc"}

    def __init__(
        self,
        roots: list[Path] | None = None,
        min_size: int = 100 * 1024,  # 100 KB
        max_files: int = 50_000,
    ):
        self.roots = roots or self.DEFAULT_ROOTS
        self.min_size = min_size
        self.max_files = max_files

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Indexing files for duplicates…")
        by_size: dict[int, list[Path]] = defaultdict(list)
        count = 0

        for root in self.roots:
            if not root.exists():
                continue
            self._report(progress, f"Walking {root}…")
            for path in self._walk(root):
                try:
                    size = path.stat().st_size
                except OSError:
                    continue
                if size < self.min_size:
                    continue
                by_size[size].append(path)
                count += 1
                if count >= self.max_files:
                    break
            if count >= self.max_files:
                break

        # Only sizes with 2+ files can be duplicates
        candidates = {s: paths for s, paths in by_size.items() if len(paths) > 1}
        self._report(progress, f"Hashing {sum(len(v) for v in candidates.values())} candidates…")

        by_hash: dict[str, list[Path]] = defaultdict(list)
        hashed = 0
        for size, paths in candidates.items():
            for path in paths:
                digest = self._quick_hash(path)
                if digest:
                    by_hash[digest].append(path)
                hashed += 1
                if hashed % 50 == 0:
                    self._report(progress, f"Hashed {hashed} files…")

        groups: list[FileGroup] = []
        dup_idx = 0
        for digest, paths in by_hash.items():
            if len(paths) < 2:
                continue
            # Deduplicate identical paths
            unique: list[Path] = []
            seen: set[str] = set()
            for p in paths:
                try:
                    key = str(p.resolve())
                except OSError:
                    key = str(p)
                if key in seen:
                    continue
                seen.add(key)
                unique.append(p)
            if len(unique) < 2:
                continue

            dup_idx += 1
            items: list[FileItem] = []
            for p in unique:
                try:
                    size = p.stat().st_size
                except OSError:
                    continue
                items.append(
                    FileItem(
                        path=p,
                        size=size,
                        category=Category.DUPLICATES,
                        reason=f"Duplicate set #{dup_idx}",
                        group_key=digest[:12],
                    )
                )
            if len(items) < 2:
                continue
            # Report reclaimable size as (n-1) * file_size (keep one)
            reclaimable = sum(i.size for i in items[1:])
            groups.append(
                FileGroup(
                    key=f"dup:{digest[:16]}",
                    category=Category.DUPLICATES,
                    title=f"{items[0].path.name} ×{len(items)}",
                    description=(
                        f"Identical files — reclaim ~{reclaimable} bytes by keeping one. "
                        f"Hash {digest[:12]}…"
                    ),
                    items=items,
                )
            )

        # Sort largest reclaimable first
        groups.sort(key=lambda g: sum(i.size for i in g.items[1:]), reverse=True)
        return groups

    def _walk(self, root: Path):
        for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
            # Prune skip dirs in-place
            dirnames[:] = [
                d
                for d in dirnames
                if d not in self.SKIP_DIRS and not d.startswith(".")
            ]
            for name in filenames:
                if name.startswith("."):
                    continue
                lower = name.lower()
                if any(lower.endswith(s) for s in self.SKIP_SUFFIXES):
                    continue
                yield Path(dirpath) / name

    def _quick_hash(self, path: Path, chunk: int = 1024 * 1024) -> str | None:
        """Hash first + last MB + size for speed; full hash if small."""
        try:
            st = path.stat()
            size = st.st_size
            h = hashlib.sha256()
            h.update(str(size).encode())
            with path.open("rb") as f:
                if size <= chunk * 2:
                    h.update(f.read())
                else:
                    h.update(f.read(chunk))
                    f.seek(max(0, size - chunk))
                    h.update(f.read(chunk))
            return h.hexdigest()
        except OSError:
            return None


class LargeOldScanner(Scanner):
    """Find large files that haven't been accessed recently."""

    name = "Large & Old Files"

    def __init__(
        self,
        roots: list[Path] | None = None,
        min_size: int = 50 * 1024 * 1024,  # 50 MB
        min_age_days: int = 90,
        limit: int = 100,
    ):
        self.roots = roots or [
            HOME / "Downloads",
            HOME / "Documents",
            HOME / "Desktop",
            HOME / "Movies",
            HOME / "Music",
            HOME / "Pictures",
        ]
        self.min_size = min_size
        self.min_age_days = min_age_days
        self.limit = limit

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        import time

        self._report(progress, "Scanning large & old files…")
        cutoff = time.time() - self.min_age_days * 86400
        found: list[FileItem] = []

        skip_dirs = DuplicateScanner.SKIP_DIRS
        for root in self.roots:
            if not root.exists():
                continue
            self._report(progress, f"Scanning {root}…")
            for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
                dirnames[:] = [
                    d for d in dirnames if d not in skip_dirs and not d.startswith(".")
                ]
                for name in filenames:
                    path = Path(dirpath) / name
                    try:
                        st = path.stat()
                    except OSError:
                        continue
                    if st.st_size < self.min_size:
                        continue
                    # Prefer atime if available, else mtime
                    age_ref = getattr(st, "st_atime", st.st_mtime)
                    if age_ref > cutoff:
                        continue
                    age_days = int((time.time() - age_ref) / 86400)
                    found.append(
                        FileItem(
                            path=path,
                            size=st.st_size,
                            category=Category.LARGE_OLD,
                            reason=f"≥{self.min_size // (1024*1024)}MB, untouched ~{age_days}d",
                            group_key=str(path),
                        )
                    )

        found.sort(key=lambda i: i.size, reverse=True)
        found = found[: self.limit]

        groups: list[FileGroup] = []
        for item in found:
            groups.append(
                FileGroup(
                    key=f"large:{item.path}",
                    category=Category.LARGE_OLD,
                    title=item.path.name,
                    description=f"{item.path.parent} — {item.reason}",
                    items=[item],
                )
            )
        return groups


class LanguageFileScanner(Scanner):
    """Find unused .lproj localization bundles in Applications (user-writable only)."""

    name = "Unused Language Files"

    # Keep these languages by default
    KEEP = {"en", "en_us", "en-us", "base", "english"}

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning language files…")
        groups: list[FileGroup] = []
        # Only scan user Applications to avoid SIP / permission issues
        apps = HOME / "Applications"
        if not apps.exists():
            return groups

        for app in list_children_safe(apps):
            if app.suffix != ".app":
                continue
            resources = app / "Contents" / "Resources"
            if not resources.exists():
                continue
            lprojs = [
                p
                for p in list_children_safe(resources)
                if p.suffix == ".lproj" and p.stem.lower() not in self.KEEP
            ]
            if not lprojs:
                continue
            items = []
            for p in lprojs:
                size = _size(p)
                if size < 100:
                    continue
                items.append(
                    FileItem(
                        path=p,
                        size=size,
                        category=Category.LANGUAGE,
                        reason=f"Localization in {app.name}",
                        group_key=app.name,
                    )
                )
            if not items:
                continue
            groups.append(
                FileGroup(
                    key=f"lang:{app.name}",
                    category=Category.LANGUAGE,
                    title=app.name,
                    description="Non-English localization bundles in user Applications",
                    items=items,
                )
            )
        return groups


def list_children_safe(path: Path) -> list[Path]:
    try:
        return sorted(path.iterdir(), key=lambda p: p.name.lower())
    except OSError:
        return []


def _size(path: Path) -> int:
    from mac_cleaner.utils import safe_size

    return safe_size(path)
