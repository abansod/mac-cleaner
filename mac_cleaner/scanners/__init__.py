from __future__ import annotations

from mac_cleaner.scanners.base import Scanner
from mac_cleaner.scanners.caches import SystemCacheScanner, TempScanner, UserCacheScanner
from mac_cleaner.scanners.downloads_browser import BrowserCacheScanner, DownloadsJunkScanner
from mac_cleaner.scanners.duplicates import DuplicateScanner, LanguageFileScanner, LargeOldScanner
from mac_cleaner.scanners.logs_trash import LogScanner, TrashScanner
from mac_cleaner.scanners.xcode_mail_leftovers import LeftoversScanner, MailScanner, XcodeScanner


def all_scanners() -> list[Scanner]:
    return [
        UserCacheScanner(),
        SystemCacheScanner(),
        LogScanner(),
        TempScanner(),
        TrashScanner(),
        DownloadsJunkScanner(),
        BrowserCacheScanner(),
        XcodeScanner(),
        MailScanner(),
        LeftoversScanner(),
        LargeOldScanner(),
        DuplicateScanner(),
        LanguageFileScanner(),
    ]


def smart_scan_scanners() -> list[Scanner]:
    """Faster subset similar to CleanMyMac Smart Scan."""
    return [
        UserCacheScanner(),
        LogScanner(),
        TempScanner(),
        TrashScanner(),
        DownloadsJunkScanner(),
        BrowserCacheScanner(),
        XcodeScanner(),
        MailScanner(),
    ]
