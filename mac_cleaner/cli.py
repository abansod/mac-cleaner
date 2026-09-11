from __future__ import annotations

import sys
from enum import Enum
from pathlib import Path
from typing import Optional

import typer
from rich.console import Console

from mac_cleaner import __version__
from mac_cleaner.engine import CleanerEngine, summarize
from mac_cleaner.models import Category
from mac_cleaner.scanners import all_scanners, smart_scan_scanners
from mac_cleaner.scanners.duplicates import DuplicateScanner
from mac_cleaner.ui.display import banner, print_groups, print_summary
from mac_cleaner.ui.interactive import InteractiveApp
from mac_cleaner.utils import format_bytes

app = typer.Typer(
    name="mac-cleaner",
    help="CleanMyMac-style CLI: junk, clutter, and duplicate cleanup for macOS.",
    add_completion=False,
    no_args_is_help=False,
)
console = Console()


class ScanMode(str, Enum):
    full = "full"
    smart = "smart"
    duplicates = "duplicates"
    junk = "junk"


@app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    version: bool = typer.Option(False, "--version", "-V", help="Show version."),
) -> None:
    if version:
        console.print(f"mac-cleaner {__version__}")
        raise typer.Exit()
    if ctx.invoked_subcommand is None:
        # Default: interactive full scan
        InteractiveApp(smart=False).run()


@app.command("scan")
def scan_cmd(
    mode: ScanMode = typer.Option(ScanMode.full, "--mode", "-m", help="Scan mode."),
    interactive: bool = typer.Option(
        True, "--interactive/--list", help="Interactive review or list-only."
    ),
) -> None:
    """Scan for junk, clutter, and duplicates."""
    if interactive and mode in {ScanMode.full, ScanMode.smart}:
        InteractiveApp(smart=mode == ScanMode.smart).run()
        return

    engine = CleanerEngine()
    if mode == ScanMode.smart:
        scanners = smart_scan_scanners()
    elif mode == ScanMode.duplicates:
        scanners = [DuplicateScanner()]
    elif mode == ScanMode.junk:
        scanners = smart_scan_scanners()
    else:
        scanners = all_scanners()

    banner()
    result = engine.scan(scanners=scanners)
    print_summary(result)
    if not result.groups:
        console.print("[green]Nothing found.[/]")
        return
    console.print()
    for cat, groups in result.by_category().items():
        console.print(f"[bold cyan]{cat.value}[/]")
        print_groups(groups)
        console.print()
    console.print(summarize(result))


@app.command("clean")
def clean_cmd(
    category: Optional[str] = typer.Option(
        None,
        "--category",
        "-c",
        help="Only clean this category name (substring match).",
    ),
    yes: bool = typer.Option(False, "--yes", "-y", help="Skip confirmation (dangerous)."),
    smart: bool = typer.Option(False, "--smart", help="Use smart scan subset."),
) -> None:
    """Non-interactive: scan then delete matching groups (with confirmation)."""
    engine = CleanerEngine()
    result = engine.scan(smart=smart)
    print_summary(result)

    groups = result.groups
    if category:
        needle = category.lower()
        groups = [
            g
            for g in groups
            if needle in g.category.value.lower() or needle in g.title.lower()
        ]
        if not groups:
            console.print(f"[yellow]No groups matched «{category}».[/]")
            raise typer.Exit(1)

    total = sum(g.size for g in groups)
    console.print(f"Will delete {len(groups)} groups ({format_bytes(total)}).")
    if not yes:
        if not typer.confirm("Proceed?"):
            raise typer.Exit(0)

    removed = freed = 0
    for g in groups:
        r, f, errors = engine.delete_group(g)
        removed += r
        freed += f
        for e in errors:
            console.print(f"[red]{e}[/]")
    console.print(f"[green]Deleted {removed} items, freed {format_bytes(freed)}.[/]")


@app.command("categories")
def categories_cmd() -> None:
    """List cleanup categories."""
    for cat in Category:
        console.print(f"  • {cat.value}")


@app.command("duplicates")
def duplicates_cmd(
    path: Optional[Path] = typer.Option(
        None, "--path", "-p", help="Extra root folder to scan for duplicates."
    ),
) -> None:
    """Find duplicate files and review interactively."""
    from mac_cleaner.scanners.duplicates import DuplicateScanner
    from mac_cleaner.ui.interactive import InteractiveApp

    banner()
    roots = list(DuplicateScanner.DEFAULT_ROOTS)
    if path:
        roots.insert(0, path.expanduser().resolve())
    engine = CleanerEngine()
    result = engine.scan(scanners=[DuplicateScanner(roots=roots)])
    print_summary(result)

    # Reuse interactive group menu via a thin wrapper
    app_ui = InteractiveApp(smart=True)
    app_ui.result = result
    if not result.groups:
        console.print("[green]No duplicates found.[/]")
        return
    # Jump straight into duplicates category
    app_ui._group_menu(Category.DUPLICATES, result.groups)  # noqa: SLF001


def run() -> None:
    if sys.platform != "darwin":
        console.print("[yellow]Warning: designed for macOS; some paths may be empty.[/]")
    app()


if __name__ == "__main__":
    run()
