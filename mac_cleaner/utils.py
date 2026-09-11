from __future__ import annotations

import os
import shutil
from pathlib import Path


HOME = Path.home()
LIBRARY = HOME / "Library"
CACHES = LIBRARY / "Caches"
LOGS = LIBRARY / "Logs"
APP_SUPPORT = LIBRARY / "Application Support"


def format_bytes(n: int) -> str:
    if n < 0:
        n = 0
    units = ["B", "KB", "MB", "GB", "TB"]
    size = float(n)
    for unit in units:
        if size < 1024 or unit == units[-1]:
            if unit == "B":
                return f"{int(size)} {unit}"
            return f"{size:.1f} {unit}"
        size /= 1024
    return f"{n} B"


def safe_size(path: Path) -> int:
    try:
        if path.is_symlink():
            return 0
        if path.is_file():
            return path.stat().st_size
        if path.is_dir():
            total = 0
            for root, dirs, files in os.walk(path, followlinks=False):
                # Skip inaccessible dirs mid-walk
                keep = []
                for d in dirs:
                    p = Path(root) / d
                    try:
                        if p.is_symlink():
                            continue
                        p.stat()
                        keep.append(d)
                    except OSError:
                        continue
                dirs[:] = keep
                for name in files:
                    fp = Path(root) / name
                    try:
                        if fp.is_symlink():
                            continue
                        total += fp.stat().st_size
                    except OSError:
                        continue
            return total
    except OSError:
        return 0
    return 0


def iter_files(path: Path, *, max_depth: int | None = None):
    """Yield file paths under path. Skips symlinks and permission errors."""
    if not path.exists():
        return
    try:
        if path.is_file() and not path.is_symlink():
            yield path
            return
    except OSError:
        return

    base_depth = len(path.parts)
    for root, dirs, files in os.walk(path, followlinks=False):
        root_path = Path(root)
        depth = len(root_path.parts) - base_depth
        if max_depth is not None and depth >= max_depth:
            dirs.clear()
        keep = []
        for d in dirs:
            p = root_path / d
            try:
                if p.is_symlink():
                    continue
                p.stat()
                keep.append(d)
            except OSError:
                continue
        dirs[:] = keep
        for name in files:
            fp = root_path / name
            try:
                if fp.is_symlink():
                    continue
                if fp.is_file():
                    yield fp
            except OSError:
                continue


def list_children(path: Path) -> list[Path]:
    if not path.exists() or not path.is_dir():
        return []
    try:
        return sorted(path.iterdir(), key=lambda p: p.name.lower())
    except OSError:
        return []


def delete_path(path: Path) -> tuple[bool, str]:
    """Delete a file or directory. Returns (ok, message)."""
    try:
        if not path.exists() and not path.is_symlink():
            return True, "already gone"
        if path.is_symlink() or path.is_file():
            path.unlink(missing_ok=True)
            return True, "deleted"
        if path.is_dir():
            shutil.rmtree(path, ignore_errors=False)
            return True, "deleted"
        return False, "unknown type"
    except PermissionError:
        return False, "permission denied"
    except OSError as e:
        return False, str(e)


PROTECTED_PREFIXES = (
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/private/var/db",
    "/Library/Apple",
)


def is_safe_to_delete(path: Path) -> bool:
    """Refuse deleting critical system paths. User-space junk is allowed."""
    try:
        resolved = path.resolve()
    except OSError:
        resolved = path
    s = str(resolved)
    home = str(HOME.resolve())
    # Always allow under home
    if s == home or s.startswith(home + os.sep):
        # But never delete the home directory itself
        if s == home:
            return False
        return True
    # Allow /private/var/folders temp (user TMPDIR area handled separately)
    for prefix in PROTECTED_PREFIXES:
        if s == prefix or s.startswith(prefix + os.sep):
            return False
    # Allow /tmp and /private/tmp contents carefully
    if s.startswith("/tmp/") or s.startswith("/private/tmp/"):
        return True
    # System caches under /Library/Caches — only if writable by user
    if s.startswith("/Library/Caches/"):
        return os.access(resolved, os.W_OK)
    return False
