from __future__ import annotations

from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from mac_cleaner.models import Category, FileGroup, ScanResult
from mac_cleaner.utils import format_bytes

console = Console()


def banner() -> None:
    console.print(
        Panel(
            Text.from_markup(
                "[bold cyan]Mac Cleaner[/]  [dim]v1.0[/]\n"
                "Junk · Clutter · Duplicates — review before you delete"
            ),
            border_style="cyan",
        )
    )


def print_summary(result: ScanResult) -> None:
    by_cat = result.by_category()
    table = Table(title="Scan Summary", show_header=True, header_style="bold")
    table.add_column("#", style="dim", width=4)
    table.add_column("Category", style="cyan")
    table.add_column("Groups", justify="right")
    table.add_column("Files", justify="right")
    table.add_column("Size", justify="right", style="green")

    cats = sorted(by_cat.keys(), key=lambda c: sum(g.size for g in by_cat[c]), reverse=True)
    for i, cat in enumerate(cats, 1):
        groups = by_cat[cat]
        files = sum(g.count for g in groups)
        size = sum(g.size for g in groups)
        table.add_row(str(i), cat.value, str(len(groups)), str(files), format_bytes(size))

    table.add_section()
    table.add_row(
        "",
        "[bold]Total[/]",
        str(len(result.groups)),
        str(result.total_files),
        f"[bold green]{format_bytes(result.total_size)}[/]",
    )
    console.print(table)


def print_groups(groups: list[FileGroup], *, start: int = 1) -> None:
    table = Table(show_header=True, header_style="bold")
    table.add_column("#", style="dim", width=4)
    table.add_column("Group", style="cyan", overflow="fold")
    table.add_column("Files", justify="right")
    table.add_column("Size", justify="right", style="green")
    table.add_column("Note", style="dim", overflow="ellipsis", max_width=40)

    for i, g in enumerate(groups, start):
        table.add_row(
            str(i),
            g.title,
            str(g.count),
            format_bytes(g.size),
            g.description[:60],
        )
    console.print(table)


def print_files(
    group: FileGroup,
    *,
    marked: set[int] | None = None,
    kept: set[int] | None = None,
) -> None:
    """Print files. marked/kept are 1-based index sets for selection UI."""
    marked = marked or set()
    kept = kept or set()
    table = Table(title=group.title, show_header=True, header_style="bold")
    table.add_column("#", style="dim", width=4)
    table.add_column("", width=3, justify="center")  # mark column
    table.add_column("Path", overflow="fold")
    table.add_column("Size", justify="right", style="green")
    table.add_column("Reason", style="dim")

    for i, item in enumerate(group.items, 1):
        status = "" if item.exists else " [red](gone)[/]"
        if i in kept:
            mark = "[green]keep[/]"
        elif i in marked:
            mark = "[red]del[/]"
        else:
            mark = ""
        table.add_row(
            str(i),
            mark,
            str(item.path) + status,
            format_bytes(item.size),
            item.reason,
        )
    console.print(table)
    console.print(
        f"[dim]{group.description} · {group.count} files · {format_bytes(group.size)}[/]"
    )


def prompt(msg: str, default: str | None = None) -> str:
    suffix = f" [{default}]" if default is not None else ""
    try:
        value = console.input(f"[bold]{msg}{suffix}:[/] ").strip()
    except (EOFError, KeyboardInterrupt):
        console.print()
        return "q"
    if not value and default is not None:
        return default
    return value


def confirm(msg: str) -> bool:
    ans = prompt(f"{msg} (y/N)", default="n").lower()
    return ans in {"y", "yes"}
