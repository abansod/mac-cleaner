from __future__ import annotations

import os
from pathlib import Path

from mac_cleaner.models import Category, FileGroup, FileItem
from mac_cleaner.scanners.base import ProgressCallback, Scanner
from mac_cleaner.utils import CACHES, list_children, safe_size

SKIP_CACHE_NAMES = {".DS_Store", "CloudKit"}


class UserCacheScanner(Scanner):
    name = "User Caches"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning user caches…")
        groups: list[FileGroup] = []
        if not CACHES.exists():
            return groups

        for child in list_children(CACHES):
            if child.name in SKIP_CACHE_NAMES:
                continue
            size = safe_size(child)
            if size < 1024:
                continue
            item = FileItem(
                path=child,
                size=size,
                category=Category.USER_CACHE,
                reason="User cache data that apps can regenerate",
                group_key=child.name,
            )
            groups.append(
                FileGroup(
                    key=f"user-cache:{child.name}",
                    category=Category.USER_CACHE,
                    title=child.name,
                    description="~/Library/Caches — safe to remove; apps rebuild as needed",
                    items=[item],
                )
            )
        return groups


class SystemCacheScanner(Scanner):
    name = "System Caches"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning system caches…")
        groups: list[FileGroup] = []
        path = Path("/Library/Caches")
        if not path.exists():
            return groups
        for child in list_children(path):
            try:
                if not child.is_dir():
                    continue
                if not os.access(child, os.W_OK):
                    continue
            except OSError:
                continue
            size = safe_size(child)
            if size < 1024 * 100:
                continue
            item = FileItem(
                path=child,
                size=size,
                category=Category.SYSTEM_CACHE,
                reason="Shared system/app cache",
                group_key=child.name,
            )
            groups.append(
                FileGroup(
                    key=f"sys-cache:{child.name}",
                    category=Category.SYSTEM_CACHE,
                    title=child.name,
                    description="/Library/Caches — only writable caches are listed",
                    items=[item],
                )
            )
        return groups


class TempScanner(Scanner):
    name = "Temporary Files"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning temporary files…")
        groups: list[FileGroup] = []
        candidates = [
            Path("/tmp"),
            Path("/private/tmp"),
            Path("/private/var/tmp"),
        ]
        tmpdir = os.environ.get("TMPDIR")
        if tmpdir:
            candidates.append(Path(tmpdir))

        seen: set[str] = set()
        for base in candidates:
            try:
                key = str(base.resolve())
            except OSError:
                key = str(base)
            if key in seen or not base.exists():
                continue
            seen.add(key)

            for child in list_children(base):
                try:
                    if not os.access(child, os.W_OK):
                        continue
                    if not (child.is_file() or child.is_dir()):
                        continue
                except OSError:
                    continue
                size = safe_size(child)
                if size < 4096:
                    continue
                item = FileItem(
                    path=child,
                    size=size,
                    category=Category.TEMP,
                    reason="Temporary file",
                    group_key=str(child),
                )
                groups.append(
                    FileGroup(
                        key=f"temp:{child}",
                        category=Category.TEMP,
                        title=child.name,
                        description=f"Temporary item in {base}",
                        items=[item],
                    )
                )
        return groups
