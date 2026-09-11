from __future__ import annotations

import time

from mac_cleaner.models import Category, FileGroup, FileItem
from mac_cleaner.scanners.base import ProgressCallback, Scanner
from mac_cleaner.utils import HOME, list_children, safe_size

INSTALLER_SUFFIXES = {
    ".dmg",
    ".pkg",
    ".zip",
    ".tar",
    ".tar.gz",
    ".tgz",
    ".iso",
    ".app.zip",
}


class DownloadsJunkScanner(Scanner):
    """Finds old installer packages and archives in Downloads."""

    name = "Installer & Download Junk"

    def __init__(self, min_age_days: int = 7, min_size: int = 1024 * 1024):
        self.min_age_days = min_age_days
        self.min_size = min_size

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning Downloads for installers…")
        groups: list[FileGroup] = []
        downloads = HOME / "Downloads"
        if not downloads.exists():
            return groups

        cutoff = time.time() - self.min_age_days * 86400
        for child in list_children(downloads):
            try:
                if not child.is_file():
                    continue
                name_lower = child.name.lower()
                if not any(name_lower.endswith(s) for s in INSTALLER_SUFFIXES):
                    continue
                st = child.stat()
                if st.st_size < self.min_size:
                    continue
                if st.st_mtime > cutoff:
                    continue
            except OSError:
                continue

            age_days = int((time.time() - st.st_mtime) / 86400)
            item = FileItem(
                path=child,
                size=st.st_size,
                category=Category.DOWNLOADS,
                reason=f"Installer/archive unused for {age_days} days",
                group_key=child.name,
            )
            groups.append(
                FileGroup(
                    key=f"dl:{child.name}",
                    category=Category.DOWNLOADS,
                    title=child.name,
                    description=f"~/Downloads — {age_days} days old",
                    items=[item],
                )
            )
        return groups


class BrowserCacheScanner(Scanner):
    name = "Browser Caches"

    BROWSER_PATHS = [
        ("Safari", HOME / "Library" / "Caches" / "com.apple.Safari"),
        ("Chrome", HOME / "Library" / "Caches" / "Google" / "Chrome"),
        ("Chrome", HOME / "Library" / "Application Support" / "Google" / "Chrome" / "Default" / "Code Cache"),
        ("Chrome", HOME / "Library" / "Application Support" / "Google" / "Chrome" / "Default" / "Service Worker" / "CacheStorage"),
        ("Firefox", HOME / "Library" / "Caches" / "Firefox"),
        ("Edge", HOME / "Library" / "Caches" / "com.microsoft.edgemac"),
        ("Brave", HOME / "Library" / "Caches" / "BraveSoftware" / "Brave-Browser"),
        ("Arc", HOME / "Library" / "Caches" / "company.thebrowser.Browser"),
    ]

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning browser caches…")
        groups: list[FileGroup] = []
        seen: set[str] = set()
        for browser, path in self.BROWSER_PATHS:
            try:
                key = str(path.resolve()) if path.exists() else str(path)
            except OSError:
                key = str(path)
            if key in seen:
                continue
            if not path.exists():
                continue
            seen.add(key)
            size = safe_size(path)
            if size < 1024 * 10:
                continue
            item = FileItem(
                path=path,
                size=size,
                category=Category.BROWSER,
                reason=f"{browser} cache",
                group_key=f"{browser}:{path.name}",
            )
            groups.append(
                FileGroup(
                    key=f"browser:{browser}:{path}",
                    category=Category.BROWSER,
                    title=f"{browser} — {path.name}",
                    description=f"{browser} cache data (pages may reload slower once)",
                    items=[item],
                )
            )
        return groups
