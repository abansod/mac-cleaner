from __future__ import annotations

from pathlib import Path

from mac_cleaner.models import Category, FileGroup, FileItem
from mac_cleaner.scanners.base import ProgressCallback, Scanner
from mac_cleaner.utils import HOME, LIBRARY, list_children, safe_size  # LIBRARY used by Mail


class XcodeScanner(Scanner):
    name = "Xcode Junk"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning Xcode junk…")
        groups: list[FileGroup] = []
        targets = [
            ("DerivedData", HOME / "Library" / "Developer" / "Xcode" / "DerivedData", "Build artifacts — safe to delete"),
            ("Archives", HOME / "Library" / "Developer" / "Xcode" / "Archives", "Old Xcode archives"),
            ("iOS DeviceSupport", HOME / "Library" / "Developer" / "Xcode" / "iOS DeviceSupport", "Device symbols"),
            ("watchOS DeviceSupport", HOME / "Library" / "Developer" / "Xcode" / "watchOS DeviceSupport", "watchOS symbols"),
            ("CoreSimulator Caches", HOME / "Library" / "Developer" / "CoreSimulator" / "Caches", "Simulator caches"),
            ("CoreSimulator Devices", HOME / "Library" / "Developer" / "CoreSimulator" / "Devices", "Simulator devices (large)"),
            ("Xcode Caches", HOME / "Library" / "Caches" / "com.apple.dt.Xcode", "Xcode app cache"),
            ("SwiftPM", HOME / "Library" / "Caches" / "org.swift.swiftpm", "Swift package cache"),
            ("CocoaPods", HOME / "Library" / "Caches" / "CocoaPods", "CocoaPods cache"),
            ("Carthage", HOME / "Library" / "Caches" / "org.carthage.CarthageKit", "Carthage cache"),
        ]

        for title, path, desc in targets:
            if not path.exists():
                continue
            # For Devices, group per device to allow selective delete
            if path.name == "Devices" and path.is_dir():
                for child in list_children(path):
                    size = safe_size(child)
                    if size < 1024 * 1024:
                        continue
                    item = FileItem(
                        path=child,
                        size=size,
                        category=Category.XCODE,
                        reason=desc,
                        group_key=child.name,
                    )
                    groups.append(
                        FileGroup(
                            key=f"xcode-sim:{child.name}",
                            category=Category.XCODE,
                            title=f"Simulator — {child.name[:8]}…",
                            description=desc,
                            items=[item],
                        )
                    )
                continue

            if "DerivedData" in title and path.is_dir():
                for child in list_children(path):
                    size = safe_size(child)
                    if size < 1024 * 100:
                        continue
                    item = FileItem(
                        path=child,
                        size=size,
                        category=Category.XCODE,
                        reason=desc,
                        group_key=child.name,
                    )
                    groups.append(
                        FileGroup(
                            key=f"xcode-dd:{child.name}",
                            category=Category.XCODE,
                            title=f"DerivedData — {child.name}",
                            description=desc,
                            items=[item],
                        )
                    )
                continue

            size = safe_size(path)
            if size < 1024 * 50:
                continue
            item = FileItem(
                path=path,
                size=size,
                category=Category.XCODE,
                reason=desc,
                group_key=title,
            )
            groups.append(
                FileGroup(
                    key=f"xcode:{title}",
                    category=Category.XCODE,
                    title=title,
                    description=desc,
                    items=[item],
                )
            )
        return groups


class MailScanner(Scanner):
    name = "Mail Downloads"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning Mail downloads…")
        groups: list[FileGroup] = []
        mail_dl = LIBRARY / "Containers" / "com.apple.mail" / "Data" / "Library" / "Mail Downloads"
        # Also classic path
        classic = HOME / "Library" / "Mail Downloads"
        for path in (mail_dl, classic):
            if not path.exists():
                continue
            for child in list_children(path):
                size = safe_size(child)
                if size < 1024:
                    continue
                item = FileItem(
                    path=child,
                    size=size,
                    category=Category.MAIL,
                    reason="Mail attachment download",
                    group_key=child.name,
                )
                groups.append(
                    FileGroup(
                        key=f"mail:{path}:{child.name}",
                        category=Category.MAIL,
                        title=child.name,
                        description="Downloaded Mail attachment",
                        items=[item],
                    )
                )
        return groups


class LeftoversScanner(Scanner):
    """Finds preference/support folders for apps that are no longer installed."""

    name = "App Leftovers"

    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        self._report(progress, "Scanning app leftovers…")
        groups: list[FileGroup] = []
        installed = self._installed_app_names()
        # Preferences
        prefs = HOME / "Library" / "Preferences"
        support = HOME / "Library" / "Application Support"
        launch_agents = HOME / "Library" / "LaunchAgents"
        saved = HOME / "Library" / "Saved Application State"

        # Heuristic: Application Support folders whose name doesn't match any .app
        if support.exists():
            for child in list_children(support):
                if not child.is_dir() or child.name.startswith("."):
                    continue
                # Skip well-known system folders
                if child.name in {
                    "AddressBook",
                    "Apple",
                    "CallHistoryDB",
                    "CallHistoryTransactions",
                    "CloudDocs",
                    "com.apple.sharedfilelist",
                    "CrashReporter",
                    "DifferentialPrivacy",
                    "DiskImages",
                    "FaceTime",
                    "FileProvider",
                    "iCloud",
                    "Knowledge",
                    "MobileSync",
                    "Network",
                    "SyncServices",
                }:
                    continue
                if self._likely_installed(child.name, installed):
                    continue
                size = safe_size(child)
                if size < 1024 * 50:
                    continue
                item = FileItem(
                    path=child,
                    size=size,
                    category=Category.LEFTOVERS,
                    reason="Application Support with no matching installed app",
                    group_key=child.name,
                )
                groups.append(
                    FileGroup(
                        key=f"left-as:{child.name}",
                        category=Category.LEFTOVERS,
                        title=child.name,
                        description="Possible leftover — verify before deleting",
                        items=[item],
                    )
                )

        # Saved Application State for missing apps
        if saved.exists():
            for child in list_children(saved):
                if not child.name.endswith(".savedState"):
                    continue
                bundle = child.name[: -len(".savedState")]
                if self._likely_installed(bundle, installed):
                    continue
                size = safe_size(child)
                if size < 1024:
                    continue
                item = FileItem(
                    path=child,
                    size=size,
                    category=Category.LEFTOVERS,
                    reason="Saved state for missing app",
                    group_key=bundle,
                )
                groups.append(
                    FileGroup(
                        key=f"left-state:{bundle}",
                        category=Category.LEFTOVERS,
                        title=bundle,
                        description="Saved Application State leftover",
                        items=[item],
                    )
                )

        _ = (prefs, launch_agents)  # reserved for future heuristics
        return groups

    def _installed_app_names(self) -> set[str]:
        names: set[str] = set()
        for apps_dir in (Path("/Applications"), HOME / "Applications"):
            if not apps_dir.exists():
                continue
            for child in list_children(apps_dir):
                if child.suffix == ".app":
                    names.add(child.stem.lower())
                    names.add(child.name.lower())
        return names

    def _likely_installed(self, folder_name: str, installed: set[str]) -> bool:
        n = folder_name.lower()
        if n in installed:
            return True
        # Match com.company.App → check if any installed name is contained
        parts = n.replace("_", ".").split(".")
        candidates = {n, parts[-1] if parts else n}
        for c in candidates:
            for app in installed:
                if c in app or app in c:
                    return True
        # Apple / system prefixes
        if n.startswith("com.apple.") or n.startswith("apple"):
            return True
        return False
