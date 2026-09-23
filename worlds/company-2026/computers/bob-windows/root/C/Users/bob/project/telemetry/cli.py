"""Command line: ``python3 -m telemetry.cli <command> [options]``."""

import argparse
import json
import sys

from telemetry import __version__
from telemetry.parse import LogError, read_samples
from telemetry.stats import DEFAULT_LIMITS, check_limits, summarize


def build_parser():
    p = argparse.ArgumentParser(prog="telemetry", description="Atlas lab log tools")
    p.add_argument("--version", action="version", version=f"telemetry {__version__}")
    sub = p.add_subparsers(dest="command", required=True)

    s = sub.add_parser("summarize", help="per-channel min/max/mean/stdev")
    s.add_argument("log", help="CSV log file")
    s.add_argument("--json", action="store_true", help="print JSON instead of a table")

    c = sub.add_parser("check", help="compare a log against the spec limits")
    c.add_argument("log", help="CSV log file")
    c.add_argument("--limit", action="append", default=[], metavar="CHANNEL=LOW:HIGH",
                   help="override a limit, written --limit=temp=-20:70")

    sub.add_parser("limits", help="print the default limits")
    return p


def parse_limit(text):
    try:
        channel, span = text.split("=", 1)
        low, high = span.split(":", 1)
        return channel.strip().lower(), (float(low), float(high))
    except ValueError:
        raise SystemExit(f"bad --limit {text!r}; expected CHANNEL=LOW:HIGH")


def cmd_summarize(args):
    summaries = summarize(read_samples(args.log))
    if args.json:
        print(json.dumps([s.__dict__ for s in summaries], indent=2))
        return 0
    print(f"{'channel':<8}{'n':>6}  {'min':>9}  {'max':>9}  {'mean':>9}  {'stdev':>8}  unit")
    for s in summaries:
        print(s.row())
    return 0


def cmd_check(args):
    limits = dict(DEFAULT_LIMITS)
    for text in args.limit:
        channel, span = parse_limit(text)
        limits[channel] = span
    failures = check_limits(summarize(read_samples(args.log)), limits)
    if not failures:
        print("PASS: every channel within limits")
        return 0
    for channel, reason in failures:
        print(f"FAIL {channel}: {reason}")
    return 1


def cmd_limits(_args):
    for channel, (low, high) in DEFAULT_LIMITS.items():
        print(f"{channel:<8}{low:>9}  {high:>9}")
    return 0


def main(argv=None):
    args = build_parser().parse_args(argv)
    handler = {"summarize": cmd_summarize, "check": cmd_check, "limits": cmd_limits}[args.command]
    try:
        return handler(args)
    except (LogError, OSError) as e:
        print(f"telemetry: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
