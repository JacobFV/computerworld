"""locale, zoneinfo and the locale-aware parts of time and datetime.

Only the locales a stock Linux has are used here, so the recorded output is
CPython's own on such a machine.
"""
import locale
import time
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo, available_timezones

LOCALES = ['C', 'en_US.UTF-8', 'en_GB.UTF-8', 'de_DE.UTF-8', 'fr_FR.UTF-8',
           'es_ES.UTF-8', 'it_IT.UTF-8', 'pt_BR.UTF-8', 'ja_JP.UTF-8',
           'zh_CN.UTF-8', 'zh_TW.UTF-8', 'ko_KR.UTF-8', 'ru_RU.UTF-8']

stamp = 1794042896  # 2026-11-07T09:14:56 UTC
tt = time.localtime(stamp)

print('--- names')
for name in LOCALES:
    print(locale.setlocale(locale.LC_ALL, name),
          time.strftime('%a|%A|%b|%B|%p', tt),
          time.strftime('%c', tt), time.strftime('%x', tt),
          time.strftime('%X', tt), sep=' ')

print('--- conventions')
for name in LOCALES:
    locale.setlocale(locale.LC_ALL, name)
    conv = locale.localeconv()
    print(name, repr(conv['decimal_point']), repr(conv['thousands_sep']),
          conv['grouping'], repr(conv['currency_symbol']), conv['frac_digits'],
          conv['p_cs_precedes'], conv['n_sign_posn'])

print('--- numbers')
for name in LOCALES:
    locale.setlocale(locale.LC_ALL, name)
    print(name,
          locale.format_string('%.2f', 1234567.891, grouping=True),
          locale.format_string('%d', -98765, grouping=True),
          locale.currency(1234.5, grouping=True),
          locale.str(0.5))

print('--- langinfo')
for name in ['de_DE.UTF-8', 'ja_JP.UTF-8', 'ru_RU.UTF-8']:
    locale.setlocale(locale.LC_ALL, name)
    print(name, locale.nl_langinfo(locale.DAY_2), locale.nl_langinfo(locale.ABDAY_2),
          locale.nl_langinfo(locale.MON_1), locale.nl_langinfo(locale.ABMON_1),
          locale.nl_langinfo(locale.AM_STR) or '-', locale.nl_langinfo(locale.D_FMT))
    print(' ', locale.getlocale(locale.LC_TIME), locale.getlocale(locale.LC_NUMERIC))

locale.setlocale(locale.LC_ALL, 'de_DE.UTF-8')
print(datetime(2026, 11, 27, 15, 34, 56).strftime('%A, %d. %B %Y um %H:%M'))
locale.setlocale(locale.LC_ALL, 'C')
try:
    locale.setlocale(locale.LC_ALL, 'xx_YY.UTF-8')
except locale.Error as e:
    print('Error:', e)

print('--- zones')
zones = ['UTC', 'Europe/Berlin', 'Europe/London', 'America/New_York',
         'America/Los_Angeles', 'Asia/Tokyo', 'Asia/Kolkata', 'Australia/Sydney',
         'America/Sao_Paulo', 'Africa/Cairo']
for key in zones:
    tz = ZoneInfo(key)
    for when in (datetime(2026, 1, 15, 12, tzinfo=timezone.utc),
                 datetime(2026, 7, 15, 12, tzinfo=timezone.utc)):
        local = when.astimezone(tz)
        print(key, local.isoformat(), local.tzname(), local.utcoffset(), local.dst())

berlin = ZoneInfo('Europe/Berlin')
ny = ZoneInfo('America/New_York')
print(datetime(2026, 7, 1, 12, tzinfo=berlin).isoformat(),
      datetime(2026, 1, 1, 12, tzinfo=berlin).isoformat())
print(datetime(2026, 7, 1, 12, tzinfo=berlin).astimezone(ny).isoformat())
print(datetime(2026, 7, 1, 12, tzinfo=berlin).timestamp(),
      datetime(2026, 7, 1, 12, tzinfo=timezone.utc).timestamp())

# The hour that happens twice, and the one that never does.
ambiguous = datetime(2026, 11, 1, 1, 30, tzinfo=ny)
print(ambiguous.isoformat(), ambiguous.replace(fold=1).isoformat())
gap = datetime(2026, 3, 8, 2, 30, tzinfo=ny)
print(gap.isoformat(), gap.astimezone(timezone.utc).isoformat())

# Arithmetic across a transition stays on the wall clock, as CPython's does.
before = datetime(2026, 3, 7, 12, tzinfo=ny)
print((before + timedelta(days=1)).isoformat(),
      (before + timedelta(days=1)).astimezone(timezone.utc).isoformat())

print(len(available_timezones()) > 50, 'Europe/Berlin' in available_timezones())
print(str(berlin), repr(berlin), berlin.key, ZoneInfo('Europe/Berlin') is berlin)
try:
    ZoneInfo('Mars/Olympus')
except Exception as e:
    print(type(e).__name__)
