"""Reading measurement logs.

A log is a CSV with a header row: ``timestamp,channel,value,unit``. Timestamps
are ISO 8601 (``2026-09-15T14:02:10``); values are decimal numbers; a channel
is a short name such as ``vin``, ``v33``, ``iload`` or ``temp``.
"""

import csv
from dataclasses import dataclass
from datetime import datetime


class LogError(ValueError):
    """A row that cannot be understood, with its line number."""


@dataclass(frozen=True)
class Sample:
    timestamp: datetime
    channel: str
    value: float
    unit: str


def parse_rows(rows, source="<rows>"):
    """Turn header + rows into Samples. Blank lines are skipped."""
    rows = iter(rows)
    try:
        header = [h.strip().lower() for h in next(rows)]
    except StopIteration:
        return []
    expected = ["timestamp", "channel", "value", "unit"]
    if header != expected:
        raise LogError(f"{source}: header must be {','.join(expected)}, got {','.join(header)}")
    samples = []
    for line_no, row in enumerate(rows, start=2):
        if not row or all(not cell.strip() for cell in row):
            continue
        if len(row) != 4:
            raise LogError(f"{source}:{line_no}: expected 4 columns, got {len(row)}")
        ts, channel, value, unit = (cell.strip() for cell in row)
        try:
            when = datetime.fromisoformat(ts)
        except ValueError:
            raise LogError(f"{source}:{line_no}: bad timestamp {ts!r}") from None
        try:
            number = float(value)
        except ValueError:
            raise LogError(f"{source}:{line_no}: bad value {value!r}") from None
        if not channel:
            raise LogError(f"{source}:{line_no}: empty channel")
        samples.append(Sample(when, channel.lower(), number, unit))
    return samples


def read_samples(path):
    """Read a CSV log from disk."""
    with open(path, newline="") as f:
        return parse_rows(csv.reader(f), source=str(path))
