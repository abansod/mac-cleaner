from __future__ import annotations

import re


def parse_index_spec(spec: str, count: int) -> tuple[set[int], str | None]:
    """
    Parse a 1-based index spec into a set of indices.

    Supports: 1 · 1,3,5 · 1-4 · 1,3-5,8 · * (all)

    Returns (indices, error_message). Indices are 1-based.
    """
    raw = spec.strip().lower()
    if not raw:
        return set(), "empty selection"
    if raw in {"*", "all"}:
        if count < 1:
            return set(), "no items"
        return set(range(1, count + 1)), None

    # Allow optional prefixes like "delete " already stripped by caller
    raw = raw.replace(" ", "")
    if not re.fullmatch(r"[\d,\-]+", raw):
        return set(), "use numbers, commas, and ranges (e.g. 1,3,5-7)"

    indices: set[int] = set()
    for part in raw.split(","):
        if not part:
            continue
        if "-" in part:
            ends = part.split("-", 1)
            if len(ends) != 2 or not ends[0] or not ends[1]:
                return set(), f"bad range «{part}»"
            try:
                start, end = int(ends[0]), int(ends[1])
            except ValueError:
                return set(), f"bad range «{part}»"
            if start > end:
                start, end = end, start
            for i in range(start, end + 1):
                indices.add(i)
        else:
            try:
                indices.add(int(part))
            except ValueError:
                return set(), f"bad number «{part}»"

    if not indices:
        return set(), "empty selection"

    bad = sorted(i for i in indices if i < 1 or i > count)
    if bad:
        return set(), f"out of range: {', '.join(map(str, bad))} (valid 1–{count})"

    return indices, None
