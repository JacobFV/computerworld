//! Excel's 1900 date system. Serial 1 is 1900-01-01 and serial 60 is the
//! nonexistent 1900-02-29 Lotus 1-2-3 introduced, which Excel keeps for compatibility;
//! from serial 61 (1900-03-01) on, a serial is days since 1899-12-30.

/// Unix day 0 (1970-01-01) as a serial.
pub const UNIX_EPOCH_SERIAL: f64 = 25_569.0;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}
const DAYS_1899_12_30: i64 = -25_569;

/// The serial of a calendar date, with month and day overflow carried as DATE() does.
/// `None` outside Excel's range (years 1900–9999).
pub fn serial(year: i64, month: i64, day: i64) -> Option<f64> {
    let total_months = year * 12 + (month - 1);
    let (y, m) = (total_months.div_euclid(12), total_months.rem_euclid(12) + 1);
    let days = days_from_civil(y, m, 1) + day - 1;
    let s = days - DAYS_1899_12_30;
    // Serials up to 60 sit before the phantom leap day.
    let s = if s <= 60 { s - 1 } else { s };
    if y == 1900 && m == 2 && day == 29 {
        return Some(60.0);
    }
    (0..=2_958_465).contains(&s).then_some(s as f64)
}
/// Calendar date of a serial (fraction ignored): (year, month, day).
pub fn ymd(serial: f64) -> Option<(i64, u32, u32)> {
    if !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let s = serial.floor() as i64;
    if s == 0 {
        return Some((1900, 1, 0));
    }
    if s == 60 {
        return Some((1900, 2, 29));
    }
    let s = if s < 60 { s + 1 } else { s };
    Some(civil_from_days(s + DAYS_1899_12_30))
}
/// Day of the week, 0 = Sunday.
pub fn weekday(serial: f64) -> u32 {
    // Serial 1 (1900-01-01) is reported as a Sunday in Excel's calendar.
    ((serial.floor() as i64 - 1).rem_euclid(7)) as u32
}
/// Hours, minutes, seconds of a serial's fraction, rounded to the nearest second.
pub fn hms(serial: f64) -> (u32, u32, u32) {
    let secs = ((serial - serial.floor()) * 86_400.0).round() as i64;
    let secs = secs.clamp(0, 86_399);
    (
        (secs / 3600) as u32,
        (secs / 60 % 60) as u32,
        (secs % 60) as u32,
    )
}
/// Milliseconds of a serial's fraction.
pub fn millis(serial: f64) -> u32 {
    let ms = ((serial - serial.floor()) * 86_400_000.0).round() as i64;
    (ms.clamp(0, 86_399_999) % 1000) as u32
}
/// A serial from microseconds since the Unix epoch (the world clock).
pub fn from_unix_micros(us: i64) -> f64 {
    let days = us.div_euclid(86_400_000_000);
    let rem = us.rem_euclid(86_400_000_000);
    UNIX_EPOCH_SERIAL + days as f64 + rem as f64 / 86_400_000_000.0
}
pub fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 31,
    }
}
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Read a date or time a person typed: `2026-09-18`, `9/18/2026`, `18-Sep-2026`,
/// `Sep 18, 2026`, `13:45`, `1:45 PM`, or a date followed by a time. Returns the serial
/// and the number format Excel would give the cell.
pub fn parse_datetime(text: &str) -> Option<(f64, &'static str)> {
    let t = text.trim();
    if let Some((d, rest)) = t.split_once(' ') {
        if let (Some(date), Some(time)) = (parse_date(d), parse_time(rest.trim())) {
            return Some((date + time, "m/d/yyyy h:mm"));
        }
    }
    if let Some(d) = parse_date(t) {
        return Some((d, "m/d/yyyy"));
    }
    parse_time(t).map(|v| {
        (
            v,
            if t.to_ascii_uppercase().ends_with('M') {
                "h:mm AM/PM"
            } else if t.matches(':').count() == 2 {
                "h:mm:ss"
            } else {
                "h:mm"
            },
        )
    })
}
fn month_named(s: &str) -> Option<i64> {
    let s = s.to_ascii_lowercase();
    if s.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| {
            m.to_ascii_lowercase().starts_with(&s)
                || s.starts_with(&m[..3].to_ascii_lowercase()) && s.len() <= m.len()
        })
        .map(|i| i as i64 + 1)
}
fn parse_date(t: &str) -> Option<f64> {
    let num = |s: &str| -> Option<i64> {
        (!s.is_empty() && s.len() <= 4 && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let valid = |y: i64, m: i64, d: i64| {
        (1..=12).contains(&m)
            && d >= 1
            && d <= i64::from(days_in_month(y, m as u32))
            && (1900..=9999).contains(&y)
    };
    let year = |y: i64, digits: usize| {
        if digits <= 2 {
            if y < 30 {
                2000 + y
            } else {
                1900 + y
            }
        } else {
            y
        }
    };
    // ISO: 2026-09-18
    let parts: Vec<&str> = t.split('-').collect();
    if parts.len() == 3 && parts[0].len() == 4 {
        let (y, m, d) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
        return valid(y, m, d).then(|| serial(y, m, d)).flatten();
    }
    // 18-Sep-2026
    if parts.len() == 3 {
        if let (Some(d), Some(m), Some(y)) = (num(parts[0]), month_named(parts[1]), num(parts[2])) {
            let y = year(y, parts[2].len());
            return valid(y, m, d).then(|| serial(y, m, d)).flatten();
        }
    }
    // US: 9/18/2026
    let parts: Vec<&str> = t.split('/').collect();
    if parts.len() == 3 {
        let (m, d, y) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
        let y = year(y, parts[2].len());
        return valid(y, m, d).then(|| serial(y, m, d)).flatten();
    }
    // Sep 18, 2026
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 3 {
        if let (Some(m), Some(d), Some(y)) = (month_named(words[0]), num(words[1]), num(words[2])) {
            return valid(y, m, d).then(|| serial(y, m, d)).flatten();
        }
    }
    None
}
fn parse_time(t: &str) -> Option<f64> {
    let upper = t.to_ascii_uppercase();
    let (body, pm) = if let Some(b) = upper.strip_suffix("PM") {
        (b.trim(), Some(true))
    } else if let Some(b) = upper.strip_suffix("AM") {
        (b.trim(), Some(false))
    } else {
        (upper.as_str(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let n = |s: &str| -> Option<f64> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
            .then(|| s.parse().ok())
            .flatten()
    };
    let mut h = n(parts[0])?;
    let m = n(parts[1])?;
    let s = if parts.len() == 3 { n(parts[2])? } else { 0.0 };
    if m >= 60.0 || s >= 60.0 {
        return None;
    }
    match pm {
        Some(true) if (1.0..12.0).contains(&h) => h += 12.0,
        Some(false) if h == 12.0 => h = 0.0,
        Some(_) if !(1.0..=12.0).contains(&h) => return None,
        _ => {}
    }
    if h >= 24.0 {
        return None;
    }
    Some((h * 3600.0 + m * 60.0 + s) / 86_400.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serials_match_excel() {
        assert_eq!(serial(1900, 1, 1), Some(1.0));
        assert_eq!(serial(1900, 2, 28), Some(59.0));
        assert_eq!(serial(1900, 2, 29), Some(60.0));
        assert_eq!(serial(1900, 3, 1), Some(61.0));
        assert_eq!(serial(2026, 9, 18), Some(46283.0));
        assert_eq!(serial(2026, 14, 1), serial(2027, 2, 1));
        assert_eq!(serial(2026, 3, 0), serial(2026, 2, 28));
        assert_eq!(ymd(46283.0), Some((2026, 9, 18)));
        assert_eq!(ymd(60.0), Some((1900, 2, 29)));
        assert_eq!(ymd(61.0), Some((1900, 3, 1)));
        // 2026-09-18 is a Friday.
        assert_eq!(weekday(46283.0), 5);
        assert_eq!(from_unix_micros(1_789_635_600_000_000), 46282.0 + 0.375);
    }
    #[test]
    fn typed_dates_and_times_are_recognised() {
        assert_eq!(parse_datetime("2026-09-18"), Some((46283.0, "m/d/yyyy")));
        assert_eq!(parse_datetime("9/18/2026").map(|x| x.0), Some(46283.0));
        assert_eq!(parse_datetime("18-Sep-2026").map(|x| x.0), Some(46283.0));
        assert_eq!(parse_datetime("Sep 18, 2026").map(|x| x.0), Some(46283.0));
        assert_eq!(parse_datetime("13:30"), Some((0.5625, "h:mm")));
        assert_eq!(parse_datetime("1:30 PM"), Some((0.5625, "h:mm AM/PM")));
        assert_eq!(parse_datetime("2/30/2026"), None);
        assert_eq!(parse_datetime("hello"), None);
    }
}
