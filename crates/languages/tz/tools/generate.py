#!/usr/bin/env python3
"""Regenerates src/data.rs from the host's IANA time zone database.

Run manually (never from a test) when the zone list changes:

    python3 crates/languages/tz/tools/generate.py

Every transition between 1970 and 2050 is recorded for each zone: the instant
(UTC seconds), the offset east of UTC in seconds, the abbreviation, and whether
it is a daylight offset. Transitions are found by walking the zone day by day
and bisecting the day where the offset changes, so the table says exactly what
the host's database says without parsing TZif ourselves.
"""
import datetime as dt
import os
import sys
from zoneinfo import ZoneInfo

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "src", "data.rs")

START = dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)
END = dt.datetime(2050, 1, 1, tzinfo=dt.timezone.utc)

# The zones the simulated world knows: every UTC offset in use, the places the
# machines and services of a world are plausibly in, and the zones whose DST
# rules a program is most likely to be tested against.
ZONES = [
    "UTC",
    "Africa/Cairo",
    "Africa/Johannesburg",
    "Africa/Lagos",
    "Africa/Nairobi",
    "America/Anchorage",
    "America/Argentina/Buenos_Aires",
    "America/Bogota",
    "America/Chicago",
    "America/Denver",
    "America/Halifax",
    "America/Lima",
    "America/Los_Angeles",
    "America/Mexico_City",
    "America/New_York",
    "America/Phoenix",
    "America/Santiago",
    "America/Sao_Paulo",
    "America/St_Johns",
    "America/Toronto",
    "America/Vancouver",
    "Asia/Bangkok",
    "Asia/Dhaka",
    "Asia/Dubai",
    "Asia/Hong_Kong",
    "Asia/Jakarta",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Kolkata",
    "Asia/Manila",
    "Asia/Riyadh",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Taipei",
    "Asia/Tehran",
    "Asia/Tokyo",
    "Atlantic/Azores",
    "Atlantic/Reykjavik",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Europe/Amsterdam",
    "Europe/Athens",
    "Europe/Berlin",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Helsinki",
    "Europe/Istanbul",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/London",
    "Europe/Madrid",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Prague",
    "Europe/Rome",
    "Europe/Stockholm",
    "Europe/Vienna",
    "Europe/Warsaw",
    "Europe/Zurich",
    "Pacific/Auckland",
    "Pacific/Fiji",
    "Pacific/Honolulu",
]

ALIASES = [
    ("Etc/UTC", "UTC"),
    ("Etc/GMT", "UTC"),
    ("GMT", "UTC"),
    ("Universal", "UTC"),
    ("Zulu", "UTC"),
    ("US/Eastern", "America/New_York"),
    ("US/Central", "America/Chicago"),
    ("US/Mountain", "America/Denver"),
    ("US/Pacific", "America/Los_Angeles"),
    ("Canada/Eastern", "America/Toronto"),
    ("Asia/Calcutta", "Asia/Kolkata"),
    ("Asia/Katmandu", "Asia/Kathmandu"),
    ("Asia/Saigon", "Asia/Bangkok"),
    ("Europe/Kiev", "Europe/Kyiv"),
    ("Australia/Canberra", "Australia/Sydney"),
    ("Australia/NSW", "Australia/Sydney"),
    ("Japan", "Asia/Tokyo"),
    ("Singapore", "Asia/Singapore"),
    ("Hongkong", "Asia/Hong_Kong"),
    ("Israel", "Asia/Jerusalem"),
    ("Turkey", "Europe/Istanbul"),
    ("Poland", "Europe/Warsaw"),
    ("Portugal", "Europe/Lisbon"),
    ("Eire", "Europe/Dublin"),
    ("GB", "Europe/London"),
    ("GB-Eire", "Europe/London"),
    ("NZ", "Pacific/Auckland"),
    ("Egypt", "Africa/Cairo"),
    ("Brazil/East", "America/Sao_Paulo"),
    ("Mexico/General", "America/Mexico_City"),
]


def state(zone, when):
    """`(offset seconds, abbreviation, is dst)` at an instant."""
    local = when.astimezone(zone)
    off = int(local.utcoffset().total_seconds())
    return off, local.tzname(), bool(local.dst())


def transitions(name):
    zone = ZoneInfo(name)
    out = [(int(START.timestamp()),) + state(zone, START)]
    day = dt.timedelta(days=1)
    when = START
    while when < END:
        nxt = when + day
        a = state(zone, when)
        b = state(zone, nxt)
        if a != b:
            # Bisect the day down to the second the change happened.
            lo, hi = when, nxt
            while (hi - lo).total_seconds() > 1:
                mid = lo + (hi - lo) / 2
                mid = mid.replace(microsecond=0)
                if state(zone, mid) == a:
                    lo = mid
                else:
                    hi = mid
            out.append((int(hi.timestamp()),) + state(zone, hi))
        when = nxt
    return out


def main():
    abbrs = []

    def abbr_index(a):
        if a not in abbrs:
            abbrs.append(a)
        return abbrs.index(a)

    rows = []
    total = 0
    for name in ZONES:
        t = transitions(name)
        total += len(t)
        parts = []
        prev = 0
        for at, off, abbr, isdst in t:
            parts.append(f"({at - prev},{off},{abbr_index(abbr)},{int(isdst)})")
            prev = at
        rows.append((name, parts))
        print(f"{name}: {len(t)} transitions", file=sys.stderr)

    with open(OUT, "w") as f:
        f.write(
            "//! Generated by `tools/generate.py` from the host's IANA database.\n"
            "//! Do not edit: rerun the generator instead.\n"
            "//!\n"
            f"//! {len(ZONES)} zones, {len(ALIASES)} aliases and {total} transitions\n"
            "//! between 1970 and 2050. A transition is `(seconds since the previous\n"
            "//! one, offset east of UTC in seconds, abbreviation index, is daylight)`.\n\n"
        )
        f.write("pub static ABBREVIATIONS: &[&str] = &[\n")
        for a in abbrs:
            f.write(f"    {a!r},\n".replace("'", '"'))
        f.write("];\n\n")
        f.write("pub static ALIASES: &[(&str, &str)] = &[\n")
        for a, b in sorted(ALIASES):
            f.write(f'    ("{a}", "{b}"),\n')
        f.write("];\n\n")
        f.write("/// `(zone, its transitions)`\n")
        f.write("pub static ZONES: &[(&str, &[crate::Transition])] = &[\n")
        for name, parts in rows:
            f.write(f'    ("{name}", &[')
            f.write(",".join(parts))
            f.write("]),\n")
        f.write("];\n")
    size = os.path.getsize(OUT)
    print(f"{OUT}: {size} bytes, {total} transitions", file=sys.stderr)


if __name__ == "__main__":
    main()
