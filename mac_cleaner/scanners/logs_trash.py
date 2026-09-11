from __future__ import annotations

from pathlib import Path

from mac_cleaner.models import Category, FileGroup, FileItem
from mac_cleaner.scanners.base import ProgressCallback, Scanner
from mac_cleaner.utils import HOME, LIBRARY, list_children, safe_size


class LogScanner(Scanner):
    name = "Logs"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning logs…")
        groups: list[FileGroup] = []
        roots = [
            LIBRARY / "Logs",
            HOME / "Library" / "Logs",
            Path("/Library/Logs"),
            Path("/private/var/log"),
        ]
        seen: set[str] = set()
        for root in roots:
            try:
                key = str(root.resolve())
            except OSError:
                key = str(root)
            if key in seen or not root.exists():
                continue
            seen.add(key)

            # Prefer grouping by top-level app/folder under Logs
            if root.is_dir():
                children = list_children(root)
                if not children:
                    continue
                for child in children:
                    if child.name.startswith("."):
                        continue
                    size = safe_size(child)
                    if size < 1024:
                        continue
                    # Skip system logs we can't write
                    try:
                        import os

                        if not os.access(child, os.W_OK):
                            continue
                    except OSError:
                        continue
                    item = FileItem(
                        path=child,
                        size=size,
                        category=Category.LOGS,
                        reason="Log files",
                        group_key=child.name,
                    )
                    groups.append(
                        FileGroup(
                            key=f"log:{root}:{child.name}",
                            category=Category.LOGS,
                            title=child.name,
                            description=f"Logs in {root}",
                            items=[item],
                        )
                    )
        return groups


class TrashScanner(Scanner):
    name = "Trash"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning Trash…")
        groups: list[FileGroup] = []
        trash = HOME / ".Trash"
        if not trash.exists():
            return groups
        for child in list_children(trash):
            size = safe_size(child)
            if size == 0 and not child.exists():
                continue
            item = FileItem(
                path=child,
                size=size,
                category=Category.TRASH,
                reason="Item in Trash",
                group_key=child.name,
            )
            groups.append(
                FileGroup(
                    key=f"trash:{child.name}",
                    category=Category.TRASH,
                    title=child.name,
                    description="~/Trash — permanently delete",
                    items=[item],
                )
            )
        return groups
