"""telemetry: parse and summarise Atlas lab measurement logs.

The sensor-node board logs one CSV row per sample (timestamp, channel, value,
unit). This package turns those logs into per-channel statistics and simple
pass/fail checks against the limits in the design spec.
"""

__version__ = "0.3.1"

from .parse import Sample, read_samples, parse_rows  # noqa: F401
from .stats import Summary, summarize, check_limits  # noqa: F401
