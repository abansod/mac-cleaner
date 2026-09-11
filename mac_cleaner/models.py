from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path


class Category(str, Enum):
    SYSTEM_CACHE = "System Caches"
    USER_CACHE = "User Caches"
    LOGS = "Logs"
    TEMP = "Temporary Files"
    TRASH = "Trash"
    DOWNLOADS = "Installer & Download Junk"
    BROWSER = "Browser Caches"
    XCODE = "Xcode Junk"
    MAIL = "Mail Downloads"
    LEFTOVERS = "App Leftovers"
    LARGE_OLD = "Large & Old Files"
    DUPLICATES = "Duplicate Files"
    LANGUAGE = "Unused Language Files"


@dataclass
class FileItem:
    path: Path
    size: int
    category: Category
    reason: str = ""
    group_key: str = ""

    @property
    def exists(self) -> bool:
        try:
            return self.path.exists()
        except OSError:
            return False


@dataclass
class FileGroup:
    """A selectable group of related files (e.g. one cache folder, one duplicate set)."""

    key: str
    category: Category
    title: str
    description: str
    items: list[FileItem] = field(default_factory=list)

    @property
    def size(self) -> int:
        return sum(i.size for i in self.items if i.exists)

    @property
    def count(self) -> int:
        return sum(1 for i in self.items if i.exists)


@dataclass
class ScanResult:
    groups: list[FileGroup] = field(default_factory=list)

    @property
    def total_size(self) -> int:
        return sum(g.size for g in self.groups)

    @property
    def total_files(self) -> int:
        return sum(g.count for g in self.groups)

    def by_category(self) -> dict[Category, list[FileGroup]]:
        out: dict[Category, list[FileGroup]] = {}
        for g in self.groups:
            out.setdefault(g.category, []).append(g)
        return out
