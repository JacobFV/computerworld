//! The locales and time zones the simulated machine carries, beyond what the
//! host that records the conformance outputs happens to have installed.
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn script(src: &str) -> Outcome {
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    cw_pyvm::run(
        &mut host,
        &Invocation {
            args: vec!["main.py".into()],
            ..Default::default()
        },
    )
}

#[test]
fn every_documented_locale_is_there() {
    let out = script(
        r#"
import locale
import time

tt = time.localtime(1794042896)
for name in ['en_US.UTF-8', 'en_GB.UTF-8', 'de_DE.UTF-8', 'fr_FR.UTF-8',
             'es_ES.UTF-8', 'it_IT.UTF-8', 'pt_BR.UTF-8', 'ja_JP.UTF-8',
             'zh_CN.UTF-8', 'zh_TW.UTF-8', 'ko_KR.UTF-8', 'ru_RU.UTF-8',
             'ar_EG.UTF-8', 'hi_IN.UTF-8']:
    locale.setlocale(locale.LC_ALL, name)
    print(name, time.strftime('%A %B', tt), locale.localeconv()['currency_symbol'])
"#,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 14, "{}", out.stdout);
    // The two the host's glibc does not have are filled in from CLDR.
    assert_eq!(lines[12], "ar_EG.UTF-8 السبت نوفمبر ج.م.");
    assert_eq!(lines[13], "hi_IN.UTF-8 शनिवार नवंबर ₹");
    assert!(
        lines[2].starts_with("de_DE.UTF-8 Samstag November €"),
        "{}",
        lines[2]
    );
}

#[test]
fn a_language_without_a_region_finds_its_locale() {
    let out = script(
        "import locale\n\
         for name in ['de', 'de_DE', 'de-DE', 'fr', 'ja', 'pt']:\n    \
             print(name, locale.setlocale(locale.LC_ALL, name))\n",
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "de de_DE.UTF-8\nde_DE de_DE.UTF-8\nde-DE de_DE.UTF-8\nfr fr_FR.UTF-8\nja ja_JP.UTF-8\npt pt_BR.UTF-8\n"
    );
}

#[test]
fn the_zone_database_covers_every_offset_in_use() {
    let out = script(
        r#"
from zoneinfo import ZoneInfo, available_timezones
from datetime import datetime, timezone

zones = sorted(available_timezones())
print(len(zones) >= 70)
offsets = set()
when = datetime(2026, 6, 15, 12, tzinfo=timezone.utc)
for key in zones:
    offsets.add(when.astimezone(ZoneInfo(key)).utcoffset())
print(len(offsets) >= 20)
# Aliases name the same data as the zone they point at.
print(ZoneInfo('US/Eastern').utcoffset(when) == ZoneInfo('America/New_York').utcoffset(when))
print(ZoneInfo('Asia/Calcutta').utcoffset(when) == ZoneInfo('Asia/Kolkata').utcoffset(when))
"#,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "True\nTrue\nTrue\nTrue\n");
}

#[test]
fn the_machines_own_clock_is_utc() {
    // A zone is something a program formats with, not something the machine is
    // in: `time.localtime` is UTC, as the world's clock is.
    let out = script(
        "import time\n\
         print(time.strftime('%Y-%m-%d %H:%M:%S %Z', time.localtime(1794042896)))\n\
         print(time.timezone, time.altzone, time.daylight, time.tzname)\n",
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "2026-11-07 09:14:56 UTC\n0 0 0 ('UTC', 'UTC')\n"
    );
}
