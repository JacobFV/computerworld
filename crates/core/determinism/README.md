# cw-determinism

Pure logical clock, stable IDs, named RNG streams and deterministic scheduling.
No host entropy, wall clock, sockets, threads or filesystem access.

RNG version `sha256-named-splitmix64-v1` derives stream state from the version tag,
little-endian seed, little-endian UTF-8 stream-name byte length, and name. SHA-256's
first eight bytes interpreted little-endian seed SplitMix64. Its fixed constants
and golden vectors are tested. Rejection sampling avoids modulo bias. Independent
stream names prevent unrelated service initialization from perturbing another
instance. Counters and stream positions are serializable.

The generic scheduler orders due microseconds, explicit phase, then insertion
sequence. Checked clock/counter arithmetic rejects overflow. Imported queues can
be validated for monotonic deadlines, unique counters and phase ordering.
