from __future__ import annotations

import re

from mac_cleaner.engine import CleanerEngine
from mac_cleaner.models import Category, FileGroup, FileItem, ScanResult
from mac_cleaner.ui.display import (
    banner,
    confirm,
    console,
    print_files,
    print_groups,
    print_summary,
    prompt,
)
from mac_cleaner.ui.selection import parse_index_spec
from mac_cleaner.utils import format_bytes


HELP = """
[bold]Commands[/]
  [cyan]1-N[/]           open category / group
  [cyan]1,3,5-7[/]       delete selected files (ranges OK)
  [cyan]k 1,3[/]         keep these files — delete everything else
  [cyan]s[/]             enter mark mode (toggle files, then delete)
  [cyan]d[/]             delete entire group (or marked files in mark mode)
  [cyan]a[/]             delete all groups in category
  [cyan]n[/] / [cyan]p[/]         next / previous page
  [cyan]r[/]             refresh / re-scan
  [cyan]b[/]             back
  [cyan]h[/]             help
  [cyan]q[/]             quit

[bold]Examples[/]
  [cyan]2,4[/]           delete files 2 and 4
  [cyan]1-3[/]           delete files 1 through 3
  [cyan]k 1[/]           keep file 1, delete the rest (great for duplicates)
  [cyan]k 1,3[/]         keep files 1 and 3, delete the rest
"""


class InteractiveApp:
    def __init__(self, *, smart: bool = False):
        self.engine = CleanerEngine()
        self.smart = smart
        self.result = ScanResult()

    def run(self) -> None:
        banner()
        self._scan()
        if not self.result.groups:
            console.print("[green]Nothing to clean — your Mac looks tidy.[/]")
            return
        self._category_menu()

    def _scan(self) -> None:
        mode = "Smart Scan" if self.smart else "Full Scan"
        console.print(f"[dim]Running {mode}…[/]")
        self.result = self.engine.scan(smart=self.smart)
        console.print()
        print_summary(self.result)
        console.print()

    def _category_menu(self) -> None:
        while True:
            by_cat = self.result.by_category()
            if not by_cat:
                console.print("[green]All clean.[/]")
                return

            cats = sorted(
                by_cat.keys(),
                key=lambda c: sum(g.size for g in by_cat[c]),
                reverse=True,
            )
            console.print(
                "[bold]Categories[/] — pick a number, or "
                "[cyan]q[/] quit / [cyan]h[/] help / [cyan]r[/] rescan"
            )
            for i, cat in enumerate(cats, 1):
                groups = by_cat[cat]
                size = sum(g.size for g in groups)
                console.print(
                    f"  [cyan]{i:2}[/]  {cat.value:<28}  "
                    f"{len(groups):3} groups  [green]{format_bytes(size):>10}[/]"
                )

            choice = prompt("Category").lower()
            if choice in {"q", "quit", "exit"}:
                return
            if choice in {"h", "help", "?"}:
                console.print(HELP)
                continue
            if choice in {"r", "rescan"}:
                self._scan()
                continue
            if not choice.isdigit():
                console.print("[red]Enter a category number.[/]")
                continue
            idx = int(choice)
            if idx < 1 or idx > len(cats):
                console.print("[red]Out of range.[/]")
                continue
            self._group_menu(cats[idx - 1], by_cat[cats[idx - 1]])

    def _group_menu(self, category: Category, groups: list[FileGroup]) -> None:
        page = 0
        page_size = 25
        while True:
            groups = [
                g for g in self.result.groups if g.category == category and g.count > 0
            ]
            if not groups:
                console.print(f"[dim]{category.value} is empty.[/]")
                return

            total_pages = max(1, (len(groups) + page_size - 1) // page_size)
            page = min(page, total_pages - 1)
            start = page * page_size
            window = groups[start : start + page_size]

            console.print()
            console.print(
                f"[bold]{category.value}[/]  "
                f"[dim]page {page + 1}/{total_pages} · {len(groups)} groups[/]"
            )
            print_groups(window, start=start + 1)
            console.print(
                "[dim]Open group # · [cyan]n[/]/[cyan]p[/]=next/prev page · "
                "[cyan]a[/]=delete all · [cyan]b[/]=back · [cyan]q[/]=quit[/]"
            )
            console.print(
                "[dim]Inside a group: [cyan]1,3[/]/[cyan]1-4[/]=delete selected · "
                "[cyan]k 1[/]=keep these (delete rest) · "
                "[cyan]s[/]=mark mode · [cyan]d[/]=delete all/marked[/]"
            )

            choice = prompt("Group").lower()
            if choice in {"q", "quit"}:
                raise SystemExit(0)
            if choice in {"b", "back"}:
                return
            if choice in {"h", "help", "?"}:
                console.print(HELP)
                continue
            if choice in {"n", "next"}:
                page = min(page + 1, total_pages - 1)
                continue
            if choice in {"p", "prev"}:
                page = max(page - 1, 0)
                continue
            if choice == "a":
                total = sum(g.size for g in groups)
                if not confirm(
                    f"Delete ALL {len(groups)} groups in {category.value} "
                    f"({format_bytes(total)})?"
                ):
                    continue
                removed = freed = 0
                errors: list[str] = []
                for g in list(groups):
                    r, f, e = self.engine.delete_group(g)
                    removed += r
                    freed += f
                    errors.extend(e)
                    self._prune_group(g)
                self._report_delete(removed, freed, errors)
                continue
            if not choice.isdigit():
                console.print("[red]Enter a group number.[/]")
                continue
            idx = int(choice)
            if idx < 1 or idx > len(groups):
                console.print("[red]Out of range.[/]")
                continue
            self._file_menu(groups[idx - 1])

    def _file_menu(self, group: FileGroup) -> None:
        marked: set[int] = set()

        while True:
            group.items = [i for i in group.items if i.exists]
            if not group.items:
                self._prune_group(group)
                console.print("[dim]Group emptied.[/]")
                return

            # Drop marks that no longer exist after deletions
            marked = {i for i in marked if 1 <= i <= len(group.items)}

            console.print()
            print_files(group, marked=marked)
            console.print(
                "[dim][cyan]# [/]/[cyan]#,# [/]/[cyan]#-#[/] delete · "
                "[cyan]k #,# [/]keep these (delete rest) · "
                "[cyan]s[/] mark mode · [cyan]d[/] delete all/marked · "
                "[cyan]b[/] back[/]"
            )
            if group.category == Category.DUPLICATES:
                console.print(
                    "[yellow]Tip:[/] [cyan]k 1[/] keeps the first copy and "
                    "deletes the rest of the duplicate set."
                )
            if marked:
                to_delete = [group.items[i - 1] for i in sorted(marked)]
                size = sum(i.size for i in to_delete)
                console.print(
                    f"[red]Marked for delete:[/] {len(marked)} file(s) "
                    f"({format_bytes(size)}) — press [cyan]d[/] to confirm"
                )

            choice = prompt("File").strip()
            low = choice.lower()

            if low in {"q", "quit"}:
                raise SystemExit(0)
            if low in {"b", "back"}:
                return
            if low in {"h", "help", "?"}:
                console.print(HELP)
                continue
            if low in {"c", "clear"}:
                marked.clear()
                console.print("[dim]Cleared marks.[/]")
                continue

            # Keep mode: k 1,3  /  keep 1  /  !1,2
            keep_spec = self._extract_keep_spec(choice)
            if keep_spec is not None:
                indices, err = parse_index_spec(keep_spec, len(group.items))
                if err:
                    console.print(f"[red]{err}[/]")
                    continue
                if not indices:
                    console.print("[red]Select at least one file to keep.[/]")
                    continue
                to_delete = [
                    item
                    for i, item in enumerate(group.items, 1)
                    if i not in indices
                ]
                if not to_delete:
                    console.print("[dim]Nothing to delete — you kept every file.[/]")
                    continue
                self._confirm_and_delete_items(
                    group,
                    to_delete,
                    kept_indices=indices,
                    action_label="keep",
                )
                marked.clear()
                continue

            # Mark mode
            if low in {"s", "select", "mark"}:
                self._mark_mode(group, marked)
                continue

            # Delete entire group, or delete marked if any
            if low == "d":
                if marked:
                    to_delete = [group.items[i - 1] for i in sorted(marked)]
                    self._confirm_and_delete_items(group, to_delete)
                    marked.clear()
                else:
                    if not confirm(
                        f"Delete entire group «{group.title}» "
                        f"({format_bytes(group.size)}, {group.count} items)?"
                    ):
                        continue
                    if group.category == Category.DUPLICATES and group.count > 1:
                        if not confirm(
                            "This removes EVERY copy in the set "
                            "(including the original). Continue?"
                        ):
                            continue
                    removed, freed, errors = self.engine.delete_group(group)
                    self._report_delete(removed, freed, errors)
                    self._prune_group(group)
                    return
                continue

            # Multi / single delete: 1  or  1,3  or  1-4  or  1,3-5
            if self._looks_like_index_spec(choice):
                indices, err = parse_index_spec(choice, len(group.items))
                if err:
                    console.print(f"[red]{err}[/]")
                    continue
                to_delete = [group.items[i - 1] for i in sorted(indices)]
                self._confirm_and_delete_items(group, to_delete)
                marked.clear()
                continue

            console.print(
                "[red]Unknown input.[/] Try [cyan]1,3[/], [cyan]k 1[/], "
                "[cyan]s[/], [cyan]d[/], or [cyan]h[/]."
            )

    def _mark_mode(self, group: FileGroup, marked: set[int]) -> None:
        """Toggle marks until user finishes with done/d/b."""
        console.print(
            "[bold]Mark mode[/] — toggle # · [cyan]*,all[/] mark all · "
            "[cyan]c[/] clear · [cyan]done[/]/[cyan]d[/] finish · [cyan]b[/] cancel"
        )
        while True:
            print_files(group, marked=marked)
            if marked:
                size = sum(group.items[i - 1].size for i in marked)
                console.print(
                    f"[red]{len(marked)} marked[/] ({format_bytes(size)})"
                )
            choice = prompt("Toggle").strip()
            low = choice.lower()
            if low in {"b", "back", "cancel"}:
                marked.clear()
                console.print("[dim]Mark mode cancelled.[/]")
                return
            if low in {"done", "d", "ok", ""}:
                return
            if low in {"c", "clear"}:
                marked.clear()
                continue
            if low in {"*", "all"}:
                marked.update(range(1, len(group.items) + 1))
                continue
            if not self._looks_like_index_spec(choice):
                console.print("[red]Enter numbers to toggle, or done.[/]")
                continue
            indices, err = parse_index_spec(choice, len(group.items))
            if err:
                console.print(f"[red]{err}[/]")
                continue
            for i in indices:
                if i in marked:
                    marked.discard(i)
                else:
                    marked.add(i)

    def _confirm_and_delete_items(
        self,
        group: FileGroup,
        items: list[FileItem],
        *,
        kept_indices: set[int] | None = None,
        action_label: str = "delete",
    ) -> None:
        if not items:
            console.print("[dim]Nothing to delete.[/]")
            return

        size = sum(i.size for i in items)
        console.print()
        if kept_indices:
            print_files(group, kept=kept_indices)
            console.print(
                f"[yellow]Will KEEP {len(kept_indices)} file(s) and "
                f"DELETE {len(items)} ({format_bytes(size)}):[/]"
            )
        else:
            console.print(
                f"[yellow]Will DELETE {len(items)} file(s) ({format_bytes(size)}):[/]"
            )
        for item in items:
            console.print(f"  [red]×[/] {item.path}  [dim]{format_bytes(item.size)}[/]")

        label = (
            f"Delete {len(items)} file(s), keep {len(kept_indices)}?"
            if kept_indices
            else f"Delete {len(items)} file(s)?"
        )
        if not confirm(label):
            return

        removed, freed, errors = self.engine.delete_items(items)
        self._report_delete(removed, freed, errors)
        group.items = [i for i in group.items if i.exists]
        if not group.items:
            self._prune_group(group)

    @staticmethod
    def _extract_keep_spec(choice: str) -> str | None:
        """Return index spec if this is a keep command, else None."""
        low = choice.strip().lower()
        for prefix in ("keep ", "k ", "except ", "!", "x "):
            if low.startswith(prefix):
                return choice.strip()[len(prefix) :].strip()
        if low in {"keep", "k", "except"}:
            return ""  # will error as empty — prompt user
        return None

    @staticmethod
    def _looks_like_index_spec(choice: str) -> bool:
        s = choice.strip().lower().replace(" ", "")
        if not s:
            return False
        if s in {"*", "all"}:
            return True
        return bool(re.fullmatch(r"[\d,\-]+", s))

    def _prune_group(self, group: FileGroup) -> None:
        group.items = [i for i in group.items if i.exists]
        if not group.items and group in self.result.groups:
            self.result.groups.remove(group)

    def _report_delete(self, removed: int, freed: int, errors: list[str]) -> None:
        if removed:
            console.print(
                f"[green]Deleted {removed} item(s), freed {format_bytes(freed)}.[/]"
            )
        for err in errors:
            console.print(f"[red]{err}[/]")
        if not removed and not errors:
            console.print("[dim]Nothing deleted.[/]")
