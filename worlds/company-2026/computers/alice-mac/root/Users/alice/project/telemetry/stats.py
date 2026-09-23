"""Per-channel statistics and limit checks."""

import statistics
from dataclasses import dataclass


@dataclass(frozen=True)
class Summary:
    channel: str
    unit: str
    count: int
    minimum: float
    maximum: float
    mean: float
    stdev: float

    def row(self):
        return (
            f"{self.channel:<8}{self.count:>6}  "
            f"{self.minimum:>9.3f}  {self.maximum:>9.3f}  "
            f"{self.mean:>9.3f}  {self.stdev:>8.4f}  {self.unit}"
        )


# Limits from docs: LDO-3v3-spec.txt and the sensor-node design review.
DEFAULT_LIMITS = {
    "v33": (3.234, 3.366),   # 3.3 V +/- 2 %
    "vin": (4.75, 5.25),     # USB VBUS
    "iload": (0.0, 0.250),   # A, regulator budget
    "temp": (-10.0, 60.0),   # degC, board surface
}


def summarize(samples):
    """One Summary per channel, in channel order."""
    by_channel = {}
    for s in samples:
        by_channel.setdefault(s.channel, []).append(s)
    out = []
    for channel in sorted(by_channel):
        group = by_channel[channel]
        values = [s.value for s in group]
        stdev = statistics.stdev(values) if len(values) > 1 else 0.0
        out.append(
            Summary(
                channel=channel,
                unit=group[0].unit,
                count=len(values),
                minimum=min(values),
                maximum=max(values),
                mean=statistics.fmean(values),
                stdev=stdev,
            )
        )
    return out


def check_limits(summaries, limits=None):
    """Return a list of (channel, reason) for every channel outside its limits."""
    limits = DEFAULT_LIMITS if limits is None else limits
    failures = []
    for s in summaries:
        if s.channel not in limits:
            continue
        low, high = limits[s.channel]
        if s.minimum < low:
            failures.append((s.channel, f"min {s.minimum:.3f} below {low}"))
        if s.maximum > high:
            failures.append((s.channel, f"max {s.maximum:.3f} above {high}"))
    return failures
