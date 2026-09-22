//! SQLite's date and time functions over the world clock. Times are milliseconds of
//! Julian day, as SQLite keeps them; the calendar is proleptic Gregorian and UTC is the
//! only zone the world has, so `localtime` and `utc` leave the time where it is.
use crate::value::Value;

const UNIX_EPOCH_JD_MS: i64 = 210_866_760_000_000;
const DAY_MS: i64 = 86_400_000;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
#[derive(Clone, Copy, Debug)]
struct Moment {
    /// Milliseconds since the Julian day epoch.
    jd: i64,
}
impl Moment {
    fn from_parts(y: i64, m: i64, d: i64, ms_of_day: i64) -> Self {
        Self {
            jd: (days_from_civil(y, m, 1) + d - 1) * DAY_MS + ms_of_day + UNIX_EPOCH_JD_MS,
        }
    }
    fn parts(self) -> (i64, i64, i64, i64) {
        let unix = self.jd - UNIX_EPOCH_JD_MS;
        let days = unix.div_euclid(DAY_MS);
        let (y, m, d) = civil_from_days(days);
        (y, m, d, unix.rem_euclid(DAY_MS))
    }
}
fn parse_number(s: &str) -> Option<f64> {
    crate::value::exact_number(s).and_then(|v| v.to_f64())
}
fn digits(s: &[u8], n: usize) -> Option<i64> {
    if s.len() < n || !s[..n].iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(&s[..n]).ok()?.parse().ok()
}
/// `HH:MM[:SS[.SSS]]` at the start of `s`: milliseconds and bytes consumed.
fn parse_hms(s: &[u8]) -> Option<(i64, usize)> {
    let h = digits(s, 2)?;
    if s.get(2) != Some(&b':') {
        return None;
    }
    let m = digits(&s[3..], 2)?;
    let mut ms = 0;
    let mut used = 5;
    if s.get(5) == Some(&b':') {
        let sec = digits(&s[6..], 2)?;
        used = 8;
        ms = sec * 1000;
        if s.get(8) == Some(&b'.') {
            let mut j = 9;
            let mut frac = String::new();
            while j < s.len() && s[j].is_ascii_digit() {
                frac.push(s[j] as char);
                j += 1;
            }
            let f: f64 = format!("0.{frac}").parse().unwrap_or(0.0);
            ms += (f * 1000.0).round() as i64;
            used = j;
        }
    }
    if h > 24 || m > 59 || ms >= 60_000 + 1000 {
        return None;
    }
    Some((h * 3_600_000 + m * 60_000 + ms, used))
}
fn parse_time(text: &str, now_us: i64) -> Option<Moment> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("now") {
        return Some(Moment {
            jd: now_us.div_euclid(1000) + UNIX_EPOCH_JD_MS,
        });
    }
    let b = t.as_bytes();
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        let y = digits(b, 4)?;
        let m = digits(&b[5..], 2)?;
        let d = digits(&b[8..], 2)?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        let mut rest = &b[10..];
        let mut ms = 0;
        if !rest.is_empty() && (rest[0] == b' ' || rest[0] == b'T') {
            let (v, used) = parse_hms(&rest[1..])?;
            ms = v;
            rest = &rest[1 + used..];
        }
        let mut moment = Moment::from_parts(y, m, d, ms);
        let rest = std::str::from_utf8(rest).ok()?.trim();
        if rest.eq_ignore_ascii_case("z") || rest.is_empty() {
            return Some(moment);
        }
        // A zone offset: the time given is local to that zone, so subtract it.
        let sign = match rest.as_bytes()[0] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let (off, used) = parse_hms(&rest.as_bytes()[1..])?;
        if 1 + used != rest.len() {
            return None;
        }
        moment.jd -= sign * off;
        return Some(moment);
    }
    if let Some((ms, used)) = parse_hms(b) {
        if used == b.len() {
            return Some(Moment::from_parts(2000, 1, 1, ms));
        }
    }
    let jd = parse_number(t)?;
    Some(Moment {
        jd: (jd * DAY_MS as f64).round() as i64,
    })
}
/// Apply modifiers in order; `None` when one is not understood.
fn apply(mut m: Moment, modifiers: &[String], first_numeric: Option<f64>) -> Option<Moment> {
    for (i, raw) in modifiers.iter().enumerate() {
        let md = raw.trim().to_ascii_lowercase();
        match md.as_str() {
            "unixepoch" if i == 0 => {
                let secs = first_numeric?;
                m.jd = (secs * 1000.0).round() as i64 + UNIX_EPOCH_JD_MS;
            }
            "julianday" if i == 0 => {
                first_numeric?;
            }
            "auto" if i == 0 => {
                if let Some(v) = first_numeric {
                    if !(0.0..5_373_484.5).contains(&v) {
                        m.jd = (v * 1000.0).round() as i64 + UNIX_EPOCH_JD_MS;
                    }
                }
            }
            "localtime" | "utc" | "subsec" | "subsecond" => {}
            "start of day" => {
                let (y, mo, d, _) = m.parts();
                m = Moment::from_parts(y, mo, d, 0);
            }
            "start of month" => {
                let (y, mo, _, _) = m.parts();
                m = Moment::from_parts(y, mo, 1, 0);
            }
            "start of year" => {
                let (y, _, _, _) = m.parts();
                m = Moment::from_parts(y, 1, 1, 0);
            }
            _ => {
                if let Some(n) = md.strip_prefix("weekday ") {
                    let n: i64 = n.trim().parse().ok().filter(|n| (0..=6).contains(n))?;
                    let (y, mo, d, ms) = m.parts();
                    let days = days_from_civil(y, mo, d);
                    let wd = (days + 4).rem_euclid(7);
                    let add = (n - wd).rem_euclid(7);
                    m = Moment::from_parts(y, mo, d + add, ms);
                    continue;
                }
                let (num, unit) = md.split_once(char::is_whitespace)?;
                let unit = unit.trim().trim_end_matches('s');
                let n: f64 = num.trim_start_matches('+').parse().ok()?;
                match unit {
                    "day" => m.jd += (n * DAY_MS as f64).round() as i64,
                    "hour" => m.jd += (n * 3_600_000.0).round() as i64,
                    "minute" => m.jd += (n * 60_000.0).round() as i64,
                    "second" => m.jd += (n * 1000.0).round() as i64,
                    "month" | "year" => {
                        let whole = n.trunc() as i64;
                        let months = if unit == "month" { whole } else { whole * 12 };
                        let (y, mo, d, ms) = m.parts();
                        let total = y * 12 + (mo - 1) + months;
                        m = Moment::from_parts(
                            total.div_euclid(12),
                            total.rem_euclid(12) + 1,
                            d,
                            ms,
                        );
                        let frac = n - n.trunc();
                        if frac != 0.0 {
                            let days = if unit == "month" {
                                frac * 30.0
                            } else {
                                frac * 365.0
                            };
                            m.jd += (days * DAY_MS as f64).round() as i64;
                        }
                    }
                    _ => return None,
                }
            }
        }
    }
    Some(m)
}
fn moment(args: &[Value], now_us: i64) -> Option<Moment> {
    let first = args.first().cloned().unwrap_or(Value::Text("now".into()));
    if first.is_null() {
        return None;
    }
    let modifiers: Vec<String> = args.iter().skip(1).map(Value::to_text).collect();
    if modifiers.iter().any(String::is_empty) && args.iter().skip(1).any(Value::is_null) {
        return None;
    }
    let numeric = match &first {
        Value::Integer(i) => Some(*i as f64),
        Value::Real(r) => Some(*r),
        Value::Text(t) => parse_number(t),
        _ => None,
    };
    let base = match (&first, numeric) {
        (Value::Integer(_) | Value::Real(_), Some(v)) => Moment {
            jd: (v * DAY_MS as f64).round() as i64,
        },
        _ => parse_time(&first.to_text(), now_us)?,
    };
    apply(base, &modifiers, numeric)
}
fn ymd(m: Moment) -> String {
    let (y, mo, d, _) = m.parts();
    if y < 0 {
        format!("-{:04}-{mo:02}-{d:02}", -y)
    } else {
        format!("{y:04}-{mo:02}-{d:02}")
    }
}
fn hms(m: Moment) -> String {
    let (_, _, _, ms) = m.parts();
    format!(
        "{:02}:{:02}:{:02}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60
    )
}
pub fn date(args: &[Value], now_us: i64) -> Value {
    moment(args, now_us).map_or(Value::Null, |m| Value::Text(ymd(m)))
}
pub fn time(args: &[Value], now_us: i64) -> Value {
    moment(args, now_us).map_or(Value::Null, |m| Value::Text(hms(m)))
}
pub fn datetime(args: &[Value], now_us: i64) -> Value {
    moment(args, now_us).map_or(Value::Null, |m| {
        Value::Text(format!("{} {}", ymd(m), hms(m)))
    })
}
pub fn julianday(args: &[Value], now_us: i64) -> Value {
    moment(args, now_us).map_or(Value::Null, |m| Value::Real(m.jd as f64 / DAY_MS as f64))
}
pub fn unixepoch(args: &[Value], now_us: i64) -> Value {
    moment(args, now_us).map_or(Value::Null, |m| {
        Value::Integer((m.jd - UNIX_EPOCH_JD_MS).div_euclid(1000))
    })
}
pub fn strftime(args: &[Value], now_us: i64) -> Value {
    let Some(format) = args.first().filter(|f| !f.is_null()).map(Value::to_text) else {
        return Value::Null;
    };
    let Some(m) = moment(&args[1..], now_us) else {
        return Value::Null;
    };
    let (y, mo, d, ms) = m.parts();
    let days = days_from_civil(y, mo, d);
    let yday = days - days_from_civil(y, 1, 1);
    let wday = (days + 4).rem_euclid(7);
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = chars.next() else {
            return Value::Null;
        };
        let h = ms / 3_600_000;
        match spec {
            'd' => out.push_str(&format!("{d:02}")),
            'e' => out.push_str(&format!("{d:2}")),
            'f' => out.push_str(&format!("{:06.3}", (ms % 60_000) as f64 / 1000.0)),
            'F' => out.push_str(&ymd(m)),
            'H' => out.push_str(&format!("{h:02}")),
            'k' => out.push_str(&format!("{h:2}")),
            'I' => out.push_str(&format!("{:02}", (h + 11) % 12 + 1)),
            'l' => out.push_str(&format!("{:2}", (h + 11) % 12 + 1)),
            'j' => out.push_str(&format!("{:03}", yday + 1)),
            'J' => out.push_str(
                crate::value::format_real(m.jd as f64 / DAY_MS as f64).trim_end_matches(".0"),
            ),
            'm' => out.push_str(&format!("{mo:02}")),
            'M' => out.push_str(&format!("{:02}", ms / 60_000 % 60)),
            'p' => out.push_str(if h < 12 { "AM" } else { "PM" }),
            'P' => out.push_str(if h < 12 { "am" } else { "pm" }),
            'R' => out.push_str(&format!("{h:02}:{:02}", ms / 60_000 % 60)),
            's' => out.push_str(&(m.jd - UNIX_EPOCH_JD_MS).div_euclid(1000).to_string()),
            'S' => out.push_str(&format!("{:02}", ms / 1000 % 60)),
            'T' => out.push_str(&hms(m)),
            'u' => out.push_str(&(if wday == 0 { 7 } else { wday }).to_string()),
            'w' => out.push_str(&wday.to_string()),
            'U' => out.push_str(&format!("{:02}", (yday + 7 - wday) / 7)),
            'W' => out.push_str(&format!("{:02}", (yday + 7 - (wday + 6) % 7) / 7)),
            'Y' => out.push_str(&format!("{y:04}")),
            '%' => out.push('%'),
            _ => return Value::Null,
        }
    }
    Value::Text(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(s: &str) -> Value {
        Value::Text(s.into())
    }
    const NOW: i64 = 1_789_635_600_000_000; // 2026-09-17 09:00:00 UTC
    #[test]
    fn dates_follow_sqlite() {
        assert_eq!(date(&[t("now")], NOW), t("2026-09-17"));
        assert_eq!(datetime(&[t("now")], NOW), t("2026-09-17 09:00:00"));
        assert_eq!(
            date(&[t("2026-01-31"), t("+1 month")], NOW),
            t("2026-03-03")
        );
        assert_eq!(
            date(
                &[
                    t("2026-09-17"),
                    t("start of month"),
                    t("+1 month"),
                    t("-1 day")
                ],
                NOW
            ),
            t("2026-09-30")
        );
        assert_eq!(
            date(&[t("2026-09-17"), t("weekday 0")], NOW),
            t("2026-09-20")
        );
        assert_eq!(unixepoch(&[t("1970-01-02")], NOW), Value::Integer(86400));
        assert_eq!(
            julianday(&[t("2000-01-01 12:00:00")], NOW),
            Value::Real(2_451_545.0)
        );
        assert_eq!(
            datetime(&[Value::Integer(0), t("unixepoch")], NOW),
            t("1970-01-01 00:00:00")
        );
        assert_eq!(
            strftime(&[t("%Y/%m/%d %H:%M %j %w"), t("2026-09-17 13:05:00")], NOW),
            t("2026/09/17 13:05 260 4")
        );
        assert_eq!(date(&[t("garbage")], NOW), Value::Null);
        assert_eq!(time(&[t("12:30")], NOW), t("12:30:00"));
    }
}
