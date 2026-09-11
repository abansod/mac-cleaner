from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import Callable

from mac_cleaner.models import FileGroup


ProgressCallback = Callable[[str], None]


class Scanner(ABC):
    name: str = "Scanner"

    @abstractmethod
    def scan(self, progress: ProgressCallback | None = None) -> list[FileGroup]:
        ...

    def _report(self, progress: ProgressCallback | None, message: str) -> None:
        if progress:
            progress(message)
