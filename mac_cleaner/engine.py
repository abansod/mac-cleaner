from __future__ import annotations

from rich.console import Console
from rich.progress import Progress, SpinnerColumn, TextColumn

from mac_cleaner.models import FileGroup, ScanResult
from mac_cleaner.scanners import all_scanners, smart_scan_scanners
from mac_cleaner.scanners.base import Scanner
from mac_cleaner.utils import delete_path, format_bytes, is_safe_to_delete


console = Console()


class CleanerEngine:
    def scan(
        self,
        *,
        smart: bool = False,
        scanners: list[Scanner] | None = None,
    ) -> ScanResult:
        scanners = scanners or (smart_scan_scanners() if smart else all_scanners())
        result = ScanResult()
        status = {"msg": "Starting…"}

        with Progress(
            SpinnerColumn(),
            TextColumn("[progress.description]{task.description}"),
            console=console,
            transient=True,
        ) as progress:
            task = progress.add_task("Scanning…", total=len(scanners))

            def on_progress(message: str) -> None:
                status["msg"] = message
                progress.update(task, description=message)

            for scanner in scanners:
                progress.update(task, description=f"{scanner.name}…")
                try:
                    groups = scanner.scan(progress=on_progress)
                    result.groups.extend(groups)
                except Exception as e:  # noqa: BLE001 — keep scanning other categories
                    console.print(f"[yellow]Warning:[/] {scanner.name} failed: {e}")
                progress.advance(task)

        # Sort groups by size descending within result
        result.groups.sort(key=lambda g: g.size, reverse=True)
        return result

    def delete_group(self, group: FileGroup) -> tuple[int, int, list[str]]:
        """Delete all items in a group. Returns (files_removed, bytes_freed, errors)."""
        return self.delete_items(group.items)

    def delete_item(self, item) -> tuple[int, int, list[str]]:
        return self.delete_items([item])

    def delete_items(self, items) -> tuple[int, int, list[str]]:
        removed = 0
        freed = 0
        errors: list[str] = []
        for item in items:
            if not item.exists:
                continue
            if not is_safe_to_delete(item.path):
                errors.append(f"Refused (protected): {item.path}")
                continue
            size = item.size
            ok, msg = delete_path(item.path)
            if ok:
                removed += 1
                freed += size
            else:
                errors.append(f"{item.path}: {msg}")
        return removed, freed, errors

    def _delete_items(self, items) -> tuple[int, int, list[str]]:
        """Deprecated alias — use delete_items."""
        return self.delete_items(items)


def summarize(result: ScanResult) -> str:
    return (
        f"Found {result.total_files} items in {len(result.groups)} groups "
        f"({format_bytes(result.total_size)})"
    )
