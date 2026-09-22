"""calendar for the simulated interpreter."""
import datetime

__all__ = ["isleap", "leapdays", "weekday", "monthrange", "monthcalendar", "month",
           "calendar", "month_name", "month_abbr", "day_name", "day_abbr", "Calendar",
           "TextCalendar", "setfirstweekday", "firstweekday", "timegm"]

MONDAY, TUESDAY, WEDNESDAY, THURSDAY, FRIDAY, SATURDAY, SUNDAY = range(7)
day_name = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
day_abbr = [d[:3] for d in day_name]
month_name = ['', 'January', 'February', 'March', 'April', 'May', 'June', 'July',
              'August', 'September', 'October', 'November', 'December']
month_abbr = [m[:3] for m in month_name]
mdays = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
_firstweekday = 0


class IllegalMonthError(ValueError):
    def __init__(self, month):
        self.month = month

    def __str__(self):
        return "bad month number %r; must be 1-12" % self.month


def isleap(year):
    return year % 4 == 0 and (year % 100 != 0 or year % 400 == 0)


def leapdays(y1, y2):
    y1 -= 1
    y2 -= 1
    return (y2 // 4 - y1 // 4) - (y2 // 100 - y1 // 100) + (y2 // 400 - y1 // 400)


def weekday(year, month, day):
    return datetime.date(year, month, day).weekday()


def monthrange(year, month):
    if not 1 <= month <= 12:
        raise IllegalMonthError(month)
    day1 = weekday(year, month, 1)
    ndays = mdays[month] + (month == 2 and isleap(year))
    return day1, ndays


def setfirstweekday(firstweekday):
    global _firstweekday
    _firstweekday = firstweekday


def firstweekday():
    return _firstweekday


def monthcalendar(year, month):
    day1, ndays = monthrange(year, month)
    offset = (day1 - _firstweekday) % 7
    days = [0] * offset + list(range(1, ndays + 1))
    days += [0] * (-len(days) % 7)
    return [days[i:i + 7] for i in range(0, len(days), 7)]


def month(theyear, themonth, w=0, l=0):
    w = max(2, w)
    l = max(1, l)
    title = f"{month_name[themonth]} {theyear}".center(7 * (w + 1) - 1).rstrip()
    lines = [title]
    names = ' '.join(day_abbr[(i + _firstweekday) % 7][:w].center(w) for i in range(7))
    lines.append(names.rstrip())
    for week in monthcalendar(theyear, themonth):
        lines.append(' '.join(('' if d == 0 else str(d)).rjust(w) for d in week).rstrip())
    return ('\n' * l).join(lines) + '\n'


def prmonth(theyear, themonth, w=0, l=0):
    print(month(theyear, themonth, w, l), end='')


def timegm(tuple):
    year, month, day, hour, minute, second = tuple[:6]
    days = datetime.date(year, month, 1).toordinal() - datetime.date(1970, 1, 1).toordinal() + day - 1
    return ((days * 24 + hour) * 60 + minute) * 60 + second


class Calendar:
    def __init__(self, firstweekday=0):
        self.firstweekday = firstweekday

    def iterweekdays(self):
        for i in range(self.firstweekday, self.firstweekday + 7):
            yield i % 7

    def itermonthdays(self, year, month):
        day1, ndays = monthrange(year, month)
        days_before = (day1 - self.firstweekday) % 7
        yield from [0] * days_before
        yield from range(1, ndays + 1)
        days_after = (self.firstweekday - day1 - ndays) % 7
        yield from [0] * days_after

    def monthdayscalendar(self, year, month):
        days = list(self.itermonthdays(year, month))
        return [days[i:i + 7] for i in range(0, len(days), 7)]

    def itermonthdates(self, year, month):
        for d in self.itermonthdays(year, month):
            if d:
                yield datetime.date(year, month, d)


class TextCalendar(Calendar):
    def formatmonth(self, theyear, themonth, w=0, l=0):
        global _firstweekday
        saved = _firstweekday
        _firstweekday = self.firstweekday
        try:
            return month(theyear, themonth, w, l)
        finally:
            _firstweekday = saved

    def prmonth(self, theyear, themonth, w=0, l=0):
        print(self.formatmonth(theyear, themonth, w, l), end='')
