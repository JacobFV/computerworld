"""zoneinfo: the IANA time zones this machine carries.

There is no `/usr/share/zoneinfo` to read: the transitions are compiled in (see
`crates/tz`), so `ZoneInfo('Europe/Berlin')` knows when that zone changes its
offset between 1970 and 2050, and a `datetime` that carries one behaves as
CPython's does.
"""
import _zoneinfo
from datetime import timedelta, tzinfo

__all__ = ['ZoneInfo', 'available_timezones', 'reset_tzpath', 'TZPATH',
           'ZoneInfoNotFoundError', 'InvalidTZPathWarning']

TZPATH = ()


class ZoneInfoNotFoundError(KeyError):
    """A zone this machine does not have."""


class InvalidTZPathWarning(RuntimeWarning):
    pass


def reset_tzpath(to=None):
    """The zones are compiled in; there is no path to search."""
    global TZPATH
    TZPATH = tuple(to) if to else ()


def available_timezones():
    return set(_zoneinfo.available())


_EPOCH_ORDINAL = 719163  # date(1970, 1, 1).toordinal()


def _to_epoch_seconds(dt):
    """Seconds from the epoch for a naive datetime read as a wall clock."""
    days = dt.toordinal() - _EPOCH_ORDINAL
    return days * 86400 + dt.hour * 3600 + dt.minute * 60 + dt.second


class ZoneInfo(tzinfo):
    _cache = {}

    def __new__(cls, key):
        if key in cls._cache:
            return cls._cache[key]
        self = cls._new(key)
        cls._cache[key] = self
        return self

    @classmethod
    def _new(cls, key):
        if not _zoneinfo.exists(key):
            raise ZoneInfoNotFoundError('No time zone found with key %s' % key)
        self = tzinfo.__new__(cls)
        self._key = key
        return self

    @classmethod
    def no_cache(cls, key):
        return cls._new(key)

    @classmethod
    def from_file(cls, fobj, key=None):
        raise ZoneInfoNotFoundError('the time zones of this machine are compiled in')

    @classmethod
    def clear_cache(cls, *, only_keys=None):
        if only_keys is None:
            cls._cache.clear()
        else:
            for k in only_keys:
                cls._cache.pop(k, None)

    @property
    def key(self):
        return self._key

    def __str__(self):
        return self._key

    def __repr__(self):
        return 'zoneinfo.ZoneInfo(key=%r)' % self._key

    def __reduce__(self):
        return (self.__class__, (self._key,))

    def _at(self, dt):
        """What the zone was doing at the instant `dt` names."""
        if dt is None:
            return None
        if dt.tzinfo is None or dt.tzinfo is self:
            # A wall clock reading in this zone.
            local = _to_epoch_seconds(dt)
            return _zoneinfo.local(self._key, local, bool(getattr(dt, 'fold', 0)))
        # An aware datetime somewhere else: convert to the instant first.
        offset = dt.utcoffset()
        local = _to_epoch_seconds(dt.replace(tzinfo=None))
        seconds = local - int(offset.total_seconds())
        return _zoneinfo.offset(self._key, seconds)

    def utcoffset(self, dt):
        info = self._at(dt)
        return None if info is None else timedelta(seconds=info[0])

    def dst(self, dt):
        info = self._at(dt)
        if info is None:
            return None
        if not info[2]:
            return timedelta(0)
        # How much of the offset is the daylight part: the difference from the
        # standard offset around it.
        standard = self._standard_offset(dt)
        return timedelta(seconds=info[0] - standard)

    def _standard_offset(self, dt):
        """The zone's offset when it is not on daylight time, near `dt`."""
        local = _to_epoch_seconds(dt.replace(tzinfo=None))
        for months in (0, 6):
            probe = local + months * 2629800
            info = _zoneinfo.offset(self._key, probe)
            if info is not None and not info[2]:
                return info[0]
        info = _zoneinfo.offset(self._key, local)
        return info[0] - 3600 if info else 0

    def tzname(self, dt):
        info = self._at(dt)
        return None if info is None else info[1]

    def fromutc(self, dt):
        if dt.tzinfo is not self:
            raise ValueError('fromutc: dt.tzinfo is not self')
        seconds = _to_epoch_seconds(dt.replace(tzinfo=None))
        info = _zoneinfo.offset(self._key, seconds)
        offset = timedelta(seconds=info[0])
        result = (dt + offset).replace(fold=0)
        # A reading that happens twice is the second one when the offset that
        # follows it is smaller.
        after = _zoneinfo.offset(self._key, seconds + 1)
        if after is not None and after[0] < info[0]:
            pass
        before = _zoneinfo.offset(self._key, seconds - 7200)
        if before is not None and before[0] > info[0]:
            result = result.replace(fold=1)
        return result
