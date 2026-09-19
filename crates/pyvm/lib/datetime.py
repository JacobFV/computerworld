"""datetime for the simulated interpreter. "Now" is the world clock (UTC)."""
import time as _time
import math as _math

MINYEAR = 1
MAXYEAR = 9999

_DAYS_IN_MONTH = [-1, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
_DAYS_BEFORE_MONTH = [-1]
_dbm = 0
for _dim in _DAYS_IN_MONTH[1:]:
    _DAYS_BEFORE_MONTH.append(_dbm)
    _dbm += _dim
del _dbm, _dim
_MONTHNAMES = [None, "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
_FULLMONTHNAMES = [None, "January", "February", "March", "April", "May", "June", "July",
                   "August", "September", "October", "November", "December"]
_DAYNAMES = [None, "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
_FULLDAYNAMES = [None, "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]


def _is_leap(year):
    return year % 4 == 0 and (year % 100 != 0 or year % 400 == 0)


def _days_before_year(year):
    y = year - 1
    return y * 365 + y // 4 - y // 100 + y // 400


def _days_in_month(year, month):
    if month == 2 and _is_leap(year):
        return 29
    return _DAYS_IN_MONTH[month]


def _days_before_month(year, month):
    return _DAYS_BEFORE_MONTH[month] + (month > 2 and _is_leap(year))


def _ymd2ord(year, month, day):
    return _days_before_year(year) + _days_before_month(year, month) + day


_DI400Y = _days_before_year(401)
_DI100Y = _days_before_year(101)
_DI4Y = _days_before_year(5)


def _ord2ymd(n):
    n -= 1
    n400, n = divmod(n, _DI400Y)
    year = n400 * 400 + 1
    n100, n = divmod(n, _DI100Y)
    n4, n = divmod(n, _DI4Y)
    n1, n = divmod(n, 365)
    year += n100 * 100 + n4 * 4 + n1
    if n1 == 4 or n100 == 4:
        return year - 1, 12, 31
    leapyear = n1 == 3 and (n4 != 24 or n100 == 3)
    month = (n + 50) >> 5
    preceding = _DAYS_BEFORE_MONTH[month] + (month > 2 and leapyear)
    if preceding > n:
        month -= 1
        preceding -= _DAYS_IN_MONTH[month] + (month == 2 and leapyear)
    n -= preceding
    return year, month, n + 1


def _check_date_fields(year, month, day):
    for v, n in ((year, 'year'), (month, 'month'), (day, 'day')):
        if not isinstance(v, int):
            raise TypeError(f"'{type(v).__name__}' object cannot be interpreted as an integer")
    if not MINYEAR <= year <= MAXYEAR:
        raise ValueError(f'year {year} is out of range')
    if not 1 <= month <= 12:
        raise ValueError('month must be in 1..12')
    dim = _days_in_month(year, month)
    if not 1 <= day <= dim:
        raise ValueError('day is out of range for month')


def _check_time_fields(hour, minute, second, microsecond):
    if not 0 <= hour <= 23:
        raise ValueError('hour must be in 0..23')
    if not 0 <= minute <= 59:
        raise ValueError('minute must be in 0..59')
    if not 0 <= second <= 59:
        raise ValueError('second must be in 0..59')
    if not 0 <= microsecond <= 999999:
        raise ValueError('microsecond must be in 0..999999')


def _format_offset(off, sep=':'):
    if off is None:
        return ''
    s = ''
    if off.days < 0:
        sign = '-'
        off = -off
    else:
        sign = '+'
    hh, mm = divmod(off, timedelta(hours=1))
    mm, ss = divmod(mm, timedelta(minutes=1))
    s += f"{sign}{hh:02d}{sep}{mm:02d}"
    if ss or ss.microseconds:
        s += f"{sep}{ss.seconds:02d}"
        if ss.microseconds:
            s += f'.{ss.microseconds:06d}'
    return s


def _strftime(fmt, tt, micro, tzinfo_obj, dt):
    out = []
    i = 0
    n = len(fmt)
    while i < n:
        ch = fmt[i]
        i += 1
        if ch != '%' or i >= n:
            out.append(ch)
            continue
        ch = fmt[i]
        i += 1
        if ch == 'f':
            out.append(f'{micro:06d}')
        elif ch == 'z':
            off = dt.utcoffset() if dt is not None and hasattr(dt, 'utcoffset') else None
            out.append(_format_offset(off, '') if off is not None else '')
        elif ch == 'Z':
            name = dt.tzname() if dt is not None and hasattr(dt, 'tzname') else None
            out.append(name or '')
        elif ch == ':' and i < n and fmt[i] == 'z':
            i += 1
            off = dt.utcoffset() if dt is not None and hasattr(dt, 'utcoffset') else None
            out.append(_format_offset(off) if off is not None else '')
        else:
            out.append(_time.strftime('%' + ch, tt))
    return ''.join(out)


class timedelta:
    __slots__ = ('_days', '_seconds', '_microseconds', '_hashcode')

    def __new__(cls, days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0):
        d = s = us = 0
        days += weeks * 7
        seconds += minutes * 60 + hours * 3600
        microseconds += milliseconds * 1000
        if isinstance(days, float):
            dayfrac, days = _math.modf(days)
            daysecondsfrac, daysecondswhole = _math.modf(dayfrac * (24. * 3600.))
            s = int(daysecondswhole)
            d = int(days)
        else:
            daysecondsfrac = 0.0
            d = days
        if isinstance(seconds, float):
            secondsfrac, seconds = _math.modf(seconds)
            seconds = int(seconds)
            secondsfrac += daysecondsfrac
        else:
            secondsfrac = daysecondsfrac
        days, seconds = divmod(seconds, 24 * 3600)
        d += days
        s += int(seconds)
        usdouble = secondsfrac * 1e6
        if isinstance(microseconds, float):
            microseconds = round(microseconds + usdouble)
            seconds, microseconds = divmod(microseconds, 1000000)
            seconds += s
            days, s = divmod(seconds, 24 * 3600)
            d += days
        else:
            microseconds = int(microseconds)
            seconds, microseconds = divmod(microseconds, 1000000)
            days, seconds = divmod(seconds, 24 * 3600)
            d += days
            s += seconds
            microseconds = round(microseconds + usdouble)
        seconds, us = divmod(microseconds, 1000000)
        s += seconds
        days, s = divmod(s, 24 * 3600)
        d += days
        if abs(d) > 999999999:
            raise OverflowError(f"days={d}; must have magnitude <= 999999999")
        self = object.__new__(cls)
        self._days = d
        self._seconds = s
        self._microseconds = us
        self._hashcode = -1
        return self

    def __repr__(self):
        args = []
        if self._days:
            args.append("days=%d" % self._days)
        if self._seconds:
            args.append("seconds=%d" % self._seconds)
        if self._microseconds:
            args.append("microseconds=%d" % self._microseconds)
        if not args:
            args.append('0')
        return "datetime.timedelta(%s)" % ', '.join(args)

    def __str__(self):
        mm, ss = divmod(self._seconds, 60)
        hh, mm = divmod(mm, 60)
        s = "%d:%02d:%02d" % (hh, mm, ss)
        if self._days:
            def plural(n):
                return n, abs(n) != 1 and "s" or ""
            s = ("%d day%s, " % plural(self._days)) + s
        if self._microseconds:
            s = s + ".%06d" % self._microseconds
        return s

    def total_seconds(self):
        return ((self.days * 86400 + self.seconds) * 10 ** 6 + self.microseconds) / 10 ** 6

    days = property(lambda self: self._days)
    seconds = property(lambda self: self._seconds)
    microseconds = property(lambda self: self._microseconds)

    def _to_microseconds(self):
        return ((self._days * (24 * 3600) + self._seconds) * 1000000 + self._microseconds)

    def __add__(self, other):
        if isinstance(other, timedelta):
            return timedelta(self._days + other._days, self._seconds + other._seconds,
                             self._microseconds + other._microseconds)
        return NotImplemented

    __radd__ = __add__

    def __sub__(self, other):
        if isinstance(other, timedelta):
            return timedelta(self._days - other._days, self._seconds - other._seconds,
                             self._microseconds - other._microseconds)
        return NotImplemented

    def __rsub__(self, other):
        if isinstance(other, timedelta):
            return -self + other
        return NotImplemented

    def __neg__(self):
        return timedelta(-self._days, -self._seconds, -self._microseconds)

    def __pos__(self):
        return self

    def __abs__(self):
        return -self if self._days < 0 else self

    def __mul__(self, other):
        if isinstance(other, int):
            return timedelta(self._days * other, self._seconds * other, self._microseconds * other)
        if isinstance(other, float):
            usec = self._to_microseconds()
            a, b = other.as_integer_ratio()
            return timedelta(0, 0, _divide_and_round(usec * a, b))
        return NotImplemented

    __rmul__ = __mul__

    def __floordiv__(self, other):
        if not isinstance(other, (int, timedelta)):
            return NotImplemented
        usec = self._to_microseconds()
        if isinstance(other, timedelta):
            return usec // other._to_microseconds()
        return timedelta(0, 0, usec // other)

    def __truediv__(self, other):
        if not isinstance(other, (int, float, timedelta)):
            return NotImplemented
        usec = self._to_microseconds()
        if isinstance(other, timedelta):
            return usec / other._to_microseconds()
        if isinstance(other, int):
            return timedelta(0, 0, _divide_and_round(usec, other))
        a, b = other.as_integer_ratio()
        return timedelta(0, 0, _divide_and_round(b * usec, a))

    def __mod__(self, other):
        if isinstance(other, timedelta):
            r = self._to_microseconds() % other._to_microseconds()
            return timedelta(0, 0, r)
        return NotImplemented

    def __divmod__(self, other):
        if isinstance(other, timedelta):
            q, r = divmod(self._to_microseconds(), other._to_microseconds())
            return q, timedelta(0, 0, r)
        return NotImplemented

    def _getstate(self):
        return (self._days, self._seconds, self._microseconds)

    def __eq__(self, other):
        if isinstance(other, timedelta):
            return self._getstate() == other._getstate()
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, timedelta):
            return self._getstate() < other._getstate()
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, timedelta):
            return self._getstate() <= other._getstate()
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, timedelta):
            return self._getstate() > other._getstate()
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, timedelta):
            return self._getstate() >= other._getstate()
        return NotImplemented

    def __hash__(self):
        return hash(self._getstate())

    def __bool__(self):
        return (self._days != 0 or self._seconds != 0 or self._microseconds != 0)


def _divide_and_round(a, b):
    q, r = divmod(a, b)
    r *= 2
    greater_than_half = r > b if b > 0 else r < b
    if greater_than_half or r == b and q % 2 == 1:
        q += 1
    return q


timedelta.min = timedelta(-999999999)
timedelta.max = timedelta(days=999999999, hours=23, minutes=59, seconds=59, microseconds=999999)
timedelta.resolution = timedelta(microseconds=1)


class date:
    def __new__(cls, year, month=None, day=None):
        _check_date_fields(year, month, day)
        self = object.__new__(cls)
        self._year = year
        self._month = month
        self._day = day
        return self

    @classmethod
    def fromtimestamp(cls, t):
        y, m, d, hh, mm, ss, weekday, jday, dst = _time.gmtime(t)
        return cls(y, m, d)

    @classmethod
    def today(cls):
        return cls.fromtimestamp(_time.time())

    @classmethod
    def fromordinal(cls, n):
        y, m, d = _ord2ymd(n)
        return cls(y, m, d)

    @classmethod
    def fromisoformat(cls, date_string):
        if not isinstance(date_string, str):
            raise TypeError('fromisoformat: argument must be str')
        s = date_string
        try:
            if len(s) == 10 and s[4] == '-' and s[7] == '-':
                return cls(int(s[0:4]), int(s[5:7]), int(s[8:10]))
            if len(s) == 8 and s.isdigit():
                return cls(int(s[0:4]), int(s[4:6]), int(s[6:8]))
        except ValueError:
            pass
        raise ValueError(f'Invalid isoformat string: {date_string!r}')

    @classmethod
    def fromisocalendar(cls, year, week, day):
        jan4 = cls(year, 1, 4)
        start = jan4.toordinal() - jan4.weekday()
        return cls.fromordinal(start + (week - 1) * 7 + day - 1)

    def __repr__(self):
        return "%s.%s(%d, %d, %d)" % (_mod(self), type(self).__qualname__, self._year, self._month, self._day)

    def ctime(self):
        weekday = self.toordinal() % 7 or 7
        return "%s %s %2d 00:00:00 %04d" % (_DAYNAMES[weekday], _MONTHNAMES[self._month], self._day, self._year)

    def timetuple(self):
        return _time.struct_time((self._year, self._month, self._day, 0, 0, 0, self.weekday(),
                                  self.toordinal() - _ymd2ord(self._year, 1, 1) + 1, -1))

    def strftime(self, fmt):
        return _strftime(fmt, self.timetuple(), 0, None, None)

    def __format__(self, fmt):
        if not isinstance(fmt, str):
            raise TypeError("must be str, not %s" % type(fmt).__name__)
        if len(fmt) != 0:
            return self.strftime(fmt)
        return str(self)

    def isoformat(self):
        return "%04d-%02d-%02d" % (self._year, self._month, self._day)

    __str__ = isoformat

    year = property(lambda self: self._year)
    month = property(lambda self: self._month)
    day = property(lambda self: self._day)

    def toordinal(self):
        return _ymd2ord(self._year, self._month, self._day)

    def replace(self, year=None, month=None, day=None):
        if year is None:
            year = self._year
        if month is None:
            month = self._month
        if day is None:
            day = self._day
        return type(self)(year, month, day)

    def _cmp(self, other):
        y, m, d = self._year, self._month, self._day
        y2, m2, d2 = other._year, other._month, other._day
        return (y, m, d) > (y2, m2, d2) and 1 or ((y, m, d) < (y2, m2, d2) and -1 or 0)

    def __eq__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime) and not isinstance(self, datetime):
            return self._cmp(other) == 0
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmp(other) <= 0
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmp(other) < 0
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmp(other) >= 0
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmp(other) > 0
        return NotImplemented

    def __hash__(self):
        return hash((self._year, self._month, self._day))

    def __add__(self, other):
        if isinstance(other, timedelta):
            o = self.toordinal() + other.days
            if 0 < o <= 3652059:
                return type(self).fromordinal(o)
            raise OverflowError("result out of range")
        return NotImplemented

    __radd__ = __add__

    def __sub__(self, other):
        if isinstance(other, timedelta):
            return self + timedelta(-other.days)
        if isinstance(other, date):
            days1 = self.toordinal()
            days2 = other.toordinal()
            return timedelta(days1 - days2)
        return NotImplemented

    def weekday(self):
        return (self.toordinal() + 6) % 7

    def isoweekday(self):
        return self.toordinal() % 7 or 7

    def isocalendar(self):
        year = self._year
        week1monday = _isoweek1monday(year)
        today = _ymd2ord(self._year, self._month, self._day)
        week, day = divmod(today - week1monday, 7)
        if week < 0:
            year -= 1
            week1monday = _isoweek1monday(year)
            week, day = divmod(today - week1monday, 7)
        elif week >= 52:
            if today >= _isoweek1monday(year + 1):
                year += 1
                week = 0
        return _IsoCalendarDate(year, week + 1, day + 1)


class _IsoCalendarDate(tuple):
    def __new__(cls, year, week, weekday):
        return tuple.__new__(cls, (year, week, weekday))

    year = property(lambda self: self[0])
    week = property(lambda self: self[1])
    weekday = property(lambda self: self[2])

    def __repr__(self):
        return f'datetime.IsoCalendarDate(year={self[0]}, week={self[1]}, weekday={self[2]})'


def _isoweek1monday(year):
    THURSDAY = 3
    firstday = _ymd2ord(year, 1, 1)
    firstweekday = (firstday + 6) % 7
    week1monday = firstday - firstweekday
    if firstweekday > THURSDAY:
        week1monday += 7
    return week1monday


def _mod(obj):
    m = type(obj).__module__
    return 'datetime' if m == 'datetime' else m


date.min = date(1, 1, 1)
date.max = date(9999, 12, 31)
date.resolution = timedelta(days=1)


class tzinfo:
    def tzname(self, dt):
        raise NotImplementedError("tzinfo subclass must override tzname()")

    def utcoffset(self, dt):
        raise NotImplementedError("tzinfo subclass must override utcoffset()")

    def dst(self, dt):
        raise NotImplementedError("tzinfo subclass must override dst()")

    def fromutc(self, dt):
        if dt.tzinfo is not self:
            raise ValueError("fromutc: dt.tzinfo is not self")
        dtoff = dt.utcoffset()
        if dtoff is None:
            raise ValueError("fromutc: non-None utcoffset() result required")
        dtdst = dt.dst()
        delta = dtoff - (dtdst or timedelta(0))
        if delta:
            dt += delta
            dtdst = dt.dst()
        return dt + (dtdst or timedelta(0))

    def __reduce__(self):
        return (self.__class__, ())


class timezone(tzinfo):
    def __new__(cls, offset, name=None):
        if not isinstance(offset, timedelta):
            raise TypeError("offset must be a timedelta")
        if not -timedelta(hours=24) < offset < timedelta(hours=24):
            raise ValueError("offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24).")
        self = object.__new__(cls)
        self._offset = offset
        self._name = name
        return self

    def __eq__(self, other):
        if isinstance(other, timezone):
            return self._offset == other._offset
        return NotImplemented

    def __hash__(self):
        return hash(self._offset)

    def __repr__(self):
        if self is timezone.utc:
            return 'datetime.timezone.utc'
        if self._name is None:
            return "datetime.timezone(%r)" % (self._offset,)
        return "datetime.timezone(%r, %r)" % (self._offset, self._name)

    def __str__(self):
        return self.tzname(None)

    def utcoffset(self, dt):
        return self._offset

    def tzname(self, dt):
        if self._name is None:
            if not self._offset:
                return 'UTC'
            return 'UTC' + _format_offset(self._offset)
        return self._name

    def dst(self, dt):
        return None

    def fromutc(self, dt):
        return dt + self._offset


timezone.utc = timezone(timedelta(0))
UTC = timezone.utc


class time:
    def __new__(cls, hour=0, minute=0, second=0, microsecond=0, tzinfo=None, *, fold=0):
        _check_time_fields(hour, minute, second, microsecond)
        self = object.__new__(cls)
        self._hour = hour
        self._minute = minute
        self._second = second
        self._microsecond = microsecond
        self._tzinfo = tzinfo
        self._fold = fold
        return self

    hour = property(lambda self: self._hour)
    minute = property(lambda self: self._minute)
    second = property(lambda self: self._second)
    microsecond = property(lambda self: self._microsecond)
    tzinfo = property(lambda self: self._tzinfo)
    fold = property(lambda self: self._fold)

    def _state(self):
        return (self._hour, self._minute, self._second, self._microsecond)

    def __eq__(self, other):
        if isinstance(other, time):
            return self._state() == other._state()
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, time):
            return self._state() < other._state()
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, time):
            return self._state() <= other._state()
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, time):
            return self._state() > other._state()
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, time):
            return self._state() >= other._state()
        return NotImplemented

    def __hash__(self):
        return hash(self._state())

    def __repr__(self):
        if self._microsecond != 0:
            s = ", %d, %d" % (self._second, self._microsecond)
        elif self._second != 0:
            s = ", %d" % self._second
        else:
            s = ""
        s = "%s.%s(%d, %d%s)" % (_mod(self), type(self).__qualname__, self._hour, self._minute, s)
        if self._tzinfo is not None:
            s = s[:-1] + ", tzinfo=%r" % self._tzinfo + ")"
        return s

    def isoformat(self, timespec='auto'):
        s = _format_time(self._hour, self._minute, self._second, self._microsecond, timespec)
        if self._tzinfo is not None:
            s += _format_offset(self._tzinfo.utcoffset(None))
        return s

    __str__ = isoformat

    @classmethod
    def fromisoformat(cls, s):
        parts = s.split(':')
        h = int(parts[0])
        m = int(parts[1]) if len(parts) > 1 else 0
        sec, us = 0, 0
        if len(parts) > 2:
            if '.' in parts[2]:
                a, b = parts[2].split('.')
                sec, us = int(a), int((b + '000000')[:6])
            else:
                sec = int(parts[2])
        return cls(h, m, sec, us)

    def strftime(self, fmt):
        tt = (1900, 1, 1, self._hour, self._minute, self._second, 0, 1, -1)
        return _strftime(fmt, tt, self._microsecond, None, None)

    def __format__(self, fmt):
        if len(fmt) != 0:
            return self.strftime(fmt)
        return str(self)

    def replace(self, hour=None, minute=None, second=None, microsecond=None, tzinfo=True, *, fold=None):
        return type(self)(self._hour if hour is None else hour,
                          self._minute if minute is None else minute,
                          self._second if second is None else second,
                          self._microsecond if microsecond is None else microsecond,
                          self._tzinfo if tzinfo is True else tzinfo,
                          fold=self._fold if fold is None else fold)

    def utcoffset(self):
        return None if self._tzinfo is None else self._tzinfo.utcoffset(None)


def _format_time(hh, mm, ss, us, timespec='auto'):
    specs = {'hours': '{:02d}', 'minutes': '{:02d}:{:02d}', 'seconds': '{:02d}:{:02d}:{:02d}',
             'milliseconds': '{:02d}:{:02d}:{:02d}.{:03d}', 'microseconds': '{:02d}:{:02d}:{:02d}.{:06d}'}
    if timespec == 'auto':
        timespec = 'microseconds' if us else 'seconds'
    elif timespec == 'milliseconds':
        us //= 1000
    try:
        fmt = specs[timespec]
    except KeyError:
        raise ValueError('Unknown timespec value')
    return fmt.format(hh, mm, ss, us)


time.min = time(0, 0, 0)
time.max = time(23, 59, 59, 999999)
time.resolution = timedelta(microseconds=1)


class datetime(date):
    def __new__(cls, year, month=None, day=None, hour=0, minute=0, second=0, microsecond=0, tzinfo=None, *, fold=0):
        _check_date_fields(year, month, day)
        _check_time_fields(hour, minute, second, microsecond)
        self = object.__new__(cls)
        self._year = year
        self._month = month
        self._day = day
        self._hour = hour
        self._minute = minute
        self._second = second
        self._microsecond = microsecond
        self._tzinfo = tzinfo
        self._fold = fold
        return self

    hour = property(lambda self: self._hour)
    minute = property(lambda self: self._minute)
    second = property(lambda self: self._second)
    microsecond = property(lambda self: self._microsecond)
    tzinfo = property(lambda self: self._tzinfo)
    fold = property(lambda self: self._fold)

    @classmethod
    def _fromtimestamp(cls, t, utc, tz):
        frac, t = _math.modf(t)
        us = round(frac * 1e6)
        if us >= 1000000:
            t += 1
            us -= 1000000
        elif us < 0:
            t -= 1
            us += 1000000
        y, m, d, hh, mm, ss, weekday, jday, dst = _time.gmtime(t)
        ss = min(ss, 59)
        result = cls(y, m, d, hh, mm, ss, us, tz)
        if tz is not None and tz is not timezone.utc:
            result = tz.fromutc(result.replace(tzinfo=tz))
        return result

    @classmethod
    def fromtimestamp(cls, t, tz=None):
        return cls._fromtimestamp(t, tz is not None, tz)

    @classmethod
    def utcfromtimestamp(cls, t):
        return cls._fromtimestamp(t, True, None)

    @classmethod
    def now(cls, tz=None):
        return cls.fromtimestamp(_time.time(), tz)

    @classmethod
    def utcnow(cls):
        return cls.utcfromtimestamp(_time.time())

    @classmethod
    def today(cls):
        return cls.now()

    @classmethod
    def combine(cls, date, time, tzinfo=True):
        if tzinfo is True:
            tzinfo = time.tzinfo
        return cls(date.year, date.month, date.day, time.hour, time.minute, time.second,
                   time.microsecond, tzinfo)

    @classmethod
    def fromisoformat(cls, s):
        if not isinstance(s, str):
            raise TypeError('fromisoformat: argument must be str')
        ds = s[:10]
        rest = s[11:] if len(s) > 10 else ''
        try:
            d = date.fromisoformat(ds)
        except ValueError:
            raise ValueError(f'Invalid isoformat string: {s!r}') from None
        tz = None
        if rest:
            if rest.endswith('Z'):
                tz = timezone.utc
                rest = rest[:-1]
            else:
                for sign in '+-':
                    if sign in rest:
                        rest, off = rest.split(sign, 1)
                        hh, mm = int(off[:2]), int(off[3:5]) if len(off) > 3 else 0
                        delta = timedelta(hours=hh, minutes=mm)
                        tz = timezone(delta if sign == '+' else -delta)
                        break
            try:
                t = time.fromisoformat(rest)
            except (ValueError, IndexError):
                raise ValueError(f'Invalid isoformat string: {s!r}') from None
        else:
            t = time()
        return cls.combine(d, t, tz)

    @classmethod
    def strptime(cls, date_string, format):
        return _strptime(cls, date_string, format)

    def timetuple(self):
        return _time.struct_time((self._year, self._month, self._day, self._hour, self._minute,
                                  self._second, self.weekday(),
                                  self.toordinal() - _ymd2ord(self._year, 1, 1) + 1, -1))

    def timestamp(self):
        days = self.toordinal() - _ymd2ord(1970, 1, 1)
        secs = days * 86400 + self._hour * 3600 + self._minute * 60 + self._second
        off = self.utcoffset()
        if off is not None:
            secs -= off.days * 86400 + off.seconds
        return secs + self._microsecond / 1e6

    def utctimetuple(self):
        return self.timetuple()

    def date(self):
        return date(self._year, self._month, self._day)

    def time(self):
        return time(self._hour, self._minute, self._second, self._microsecond)

    def timetz(self):
        return time(self._hour, self._minute, self._second, self._microsecond, self._tzinfo)

    def replace(self, year=None, month=None, day=None, hour=None, minute=None, second=None,
                microsecond=None, tzinfo=True, *, fold=None):
        return type(self)(self._year if year is None else year,
                          self._month if month is None else month,
                          self._day if day is None else day,
                          self._hour if hour is None else hour,
                          self._minute if minute is None else minute,
                          self._second if second is None else second,
                          self._microsecond if microsecond is None else microsecond,
                          self._tzinfo if tzinfo is True else tzinfo,
                          fold=self._fold if fold is None else fold)

    def astimezone(self, tz=None):
        if tz is None:
            tz = timezone.utc
        off = self.utcoffset()
        if off is None:
            # A naive datetime is read as local time, which here is UTC.
            off = timedelta(0)
        if self._tzinfo is tz:
            return self
        # The instant, handed to the zone to place on its own clock: a zone
        # whose offset changes has to decide that itself (`fromutc`).
        utc = (self - off).replace(tzinfo=tz)
        return tz.fromutc(utc)

    def ctime(self):
        weekday = self.toordinal() % 7 or 7
        return "%s %s %2d %02d:%02d:%02d %04d" % (_DAYNAMES[weekday], _MONTHNAMES[self._month],
                                                   self._day, self._hour, self._minute,
                                                   self._second, self._year)

    def isoformat(self, sep='T', timespec='auto'):
        s = ("%04d-%02d-%02d%c" % (self._year, self._month, self._day, sep) +
             _format_time(self._hour, self._minute, self._second, self._microsecond, timespec))
        off = self.utcoffset()
        if off is not None:
            s += _format_offset(off)
        return s

    def __repr__(self):
        L = [self._year, self._month, self._day, self._hour, self._minute, self._second, self._microsecond]
        if L[-1] == 0:
            del L[-1]
        if L[-1] == 0:
            del L[-1]
        s = "%s.%s(%s)" % (_mod(self), type(self).__qualname__, ", ".join(map(str, L)))
        if self._tzinfo is not None:
            s = s[:-1] + ", tzinfo=%r" % self._tzinfo + ")"
        return s

    def __str__(self):
        return self.isoformat(sep=' ')

    def strftime(self, fmt):
        return _strftime(fmt, self.timetuple(), self._microsecond, self._tzinfo, self)

    def utcoffset(self):
        if self._tzinfo is None:
            return None
        return self._tzinfo.utcoffset(self)

    def tzname(self):
        if self._tzinfo is None:
            return None
        return self._tzinfo.tzname(self)

    def dst(self):
        if self._tzinfo is None:
            return None
        return self._tzinfo.dst(self)

    def _key(self):
        base = (self._year, self._month, self._day, self._hour, self._minute, self._second, self._microsecond)
        off = self.utcoffset()
        if off is None:
            return base, None
        t = self - off
        return (t._year, t._month, t._day, t._hour, t._minute, t._second, t._microsecond), 0

    def _cmpkey(self, other):
        a, ao = self._key()
        b, bo = other._key()
        if (ao is None) != (bo is None):
            raise TypeError("can't compare offset-naive and offset-aware datetimes")
        return a, b

    def __eq__(self, other):
        if isinstance(other, datetime):
            try:
                a, b = self._cmpkey(other)
            except TypeError:
                return False
            return a == b
        if isinstance(other, date):
            return False
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, datetime):
            a, b = self._cmpkey(other)
            return a < b
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, datetime):
            a, b = self._cmpkey(other)
            return a <= b
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, datetime):
            a, b = self._cmpkey(other)
            return a > b
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, datetime):
            a, b = self._cmpkey(other)
            return a >= b
        return NotImplemented

    def __hash__(self):
        return hash(self._key()[0])

    def __add__(self, other):
        if not isinstance(other, timedelta):
            return NotImplemented
        delta = timedelta(self.toordinal(), hours=self._hour, minutes=self._minute,
                          seconds=self._second, microseconds=self._microsecond)
        delta += other
        hour, rem = divmod(delta.seconds, 3600)
        minute, second = divmod(rem, 60)
        if 0 < delta.days <= 3652059:
            return type(self).combine(date.fromordinal(delta.days),
                                      time(hour, minute, second, delta.microseconds, tzinfo=self._tzinfo))
        raise OverflowError("result out of range")

    __radd__ = __add__

    def __sub__(self, other):
        if not isinstance(other, datetime):
            if isinstance(other, timedelta):
                return self + -other
            return NotImplemented
        days1 = self.toordinal()
        days2 = other.toordinal()
        secs1 = self._second + self._minute * 60 + self._hour * 3600
        secs2 = other._second + other._minute * 60 + other._hour * 3600
        base = timedelta(days1 - days2, secs1 - secs2, self._microsecond - other._microsecond)
        myoff = self.utcoffset()
        otoff = other.utcoffset()
        if myoff == otoff:
            return base
        if myoff is None or otoff is None:
            raise TypeError("cannot mix naive and timezone-aware time")
        return base + otoff - myoff


datetime.min = datetime(1, 1, 1)
datetime.max = datetime(9999, 12, 31, 23, 59, 59, 999999)
datetime.resolution = timedelta(microseconds=1)


def _strptime(cls, data, fmt):
    import re
    directives = {
        'Y': r'(?P<Y>\d{4})', 'm': r'(?P<m>1[0-2]|0[1-9]|[1-9])', 'd': r'(?P<d>3[01]|[12]\d|0[1-9]|[1-9]| [1-9])',
        'H': r'(?P<H>2[0-3]|[0-1]\d|\d)', 'M': r'(?P<M>[0-5]\d|\d)', 'S': r'(?P<S>6[0-1]|[0-5]\d|\d)',
        'f': r'(?P<f>[0-9]{1,6})', 'y': r'(?P<y>\d\d)', 'I': r'(?P<I>1[0-2]|0[1-9]|[1-9])',
        'p': r'(?P<p>am|pm|AM|PM)', 'b': r'(?P<b>[A-Za-z]{3})', 'B': r'(?P<B>[A-Za-z]+)',
        'a': r'(?P<a>[A-Za-z]{3})', 'A': r'(?P<A>[A-Za-z]+)', 'j': r'(?P<j>36[0-6]|3[0-5]\d|[12]\d\d|0[1-9]\d|00[1-9]|[1-9]\d|0[1-9]|[1-9])',
        'z': r'(?P<z>[+-]\d\d:?[0-5]\d|Z)', 'Z': r'(?P<Z>UTC|GMT)', '%': '%',
    }
    pattern = ''
    i = 0
    while i < len(fmt):
        c = fmt[i]
        if c == '%' and i + 1 < len(fmt):
            d = fmt[i + 1]
            if d not in directives:
                raise ValueError(f"'{d}' is a bad directive in format '{fmt}'")
            pattern += directives[d]
            i += 2
        else:
            pattern += re.escape(c) if not c.isspace() else r'\s+'
            i += 1
    m = re.match('(?i)' + pattern + '$', data)
    if not m:
        raise ValueError(f"time data {data!r} does not match format {fmt!r}")
    g = m.groupdict()
    year = int(g['Y']) if g.get('Y') else (2000 + int(g['y']) if int(g['y']) < 69 else 1900 + int(g['y'])) if g.get('y') else 1900
    month = int(g['m']) if g.get('m') else 1
    if g.get('b'):
        month = [n.lower() if n else n for n in _MONTHNAMES].index(g['b'].lower())
    if g.get('B'):
        month = [n.lower() if n else n for n in _FULLMONTHNAMES].index(g['B'].lower())
    day = int(g['d']) if g.get('d') else 1
    hour = int(g['H']) if g.get('H') else 0
    if g.get('I'):
        hour = int(g['I']) % 12
        if g.get('p') and g['p'].lower() == 'pm':
            hour += 12
    minute = int(g['M']) if g.get('M') else 0
    second = int(g['S']) if g.get('S') else 0
    micro = int((g['f'] + '000000')[:6]) if g.get('f') else 0
    tz = None
    if g.get('z'):
        z = g['z']
        if z == 'Z':
            tz = timezone.utc
        else:
            z = z.replace(':', '')
            delta = timedelta(hours=int(z[1:3]), minutes=int(z[3:5]))
            tz = timezone(-delta if z[0] == '-' else delta)
    if g.get('j') and not g.get('m'):
        dt = date(year, 1, 1) + timedelta(int(g['j']) - 1)
        month, day = dt.month, dt.day
    result = datetime(year, month, day, hour, minute, second, micro, tz)
    if cls is date:
        return result.date()
    return result
