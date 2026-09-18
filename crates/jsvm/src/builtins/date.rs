//! `Date` (the machine's local time zone is UTC).

use super::*;
use crate::value::*;
use crate::vm::Vm;

const MS_DAY: f64 = 86_400_000.0;
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub const MONTHS_LONG: [&str; 12] = [
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
pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const DAYS_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Clone, Copy, Debug)]
pub struct Parts {
    pub year: i64,
    pub month: i64, // 0-11
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub ms: i64,
    pub weekday: i64,
}

pub fn parts(t: f64) -> Parts {
    let days = (t / MS_DAY).floor() as i64;
    let rem = (t - days as f64 * MS_DAY) as i64;
    let (y, m, d) = civil_from_days(days);
    Parts {
        year: y,
        month: m - 1,
        day: d,
        hour: rem / 3_600_000,
        minute: (rem / 60_000) % 60,
        second: (rem / 1000) % 60,
        ms: rem % 1000,
        weekday: (days + 4).rem_euclid(7),
    }
}

pub fn make_time(year: f64, month: f64, day: f64, h: f64, mi: f64, s: f64, ms: f64) -> f64 {
    if ![year, month, day, h, mi, s, ms]
        .iter()
        .all(|x| x.is_finite())
    {
        return f64::NAN;
    }
    let ym = year + (month / 12.0).floor();
    let mn = month.rem_euclid(12.0);
    let days = days_from_civil(ym as i64, mn as i64 + 1, 1) as f64 + day.trunc() - 1.0;
    let t = days * MS_DAY
        + h.trunc() * 3_600_000.0
        + mi.trunc() * 60_000.0
        + s.trunc() * 1000.0
        + ms.trunc();
    time_clip(t)
}

pub fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        f64::NAN
    } else {
        t.trunc() + 0.0
    }
}

fn year_str(y: i64) -> String {
    if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else if y < 0 {
        format!("-{:06}", -y)
    } else {
        format!("+{y:06}")
    }
}

pub fn iso_string(t: f64) -> String {
    let p = parts(t);
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year_str(p.year),
        p.month + 1,
        p.day,
        p.hour,
        p.minute,
        p.second,
        p.ms
    )
}

pub fn date_to_string(t: f64) -> String {
    if t.is_nan() {
        return "Invalid Date".into();
    }
    let p = parts(t);
    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        DAYS[p.weekday as usize],
        MONTHS[p.month as usize],
        p.day,
        yr4(p.year),
        p.hour,
        p.minute,
        p.second
    )
}

fn yr4(y: i64) -> String {
    if y < 0 {
        format!("-{:06}", -y)
    } else {
        format!("{y:04}")
    }
}

/// Parses the formats V8 accepts in practice.
pub fn parse_date(s: &str) -> f64 {
    let s = s.trim();
    if let Some(t) = parse_iso(s) {
        return t;
    }
    parse_loose(s).unwrap_or(f64::NAN)
}

fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    let num = |i: &mut usize, n: usize| -> Option<i64> {
        if *i + n > b.len() || !b[*i..*i + n].iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let v: i64 = s[*i..*i + n].parse().ok()?;
        *i += n;
        Some(v)
    };
    let year = if b.first() == Some(&b'+') || b.first() == Some(&b'-') {
        let neg = b[0] == b'-';
        i = 1;
        let y = num(&mut i, 6)?;
        if neg {
            -y
        } else {
            y
        }
    } else {
        num(&mut i, 4)?
    };
    let mut month = 1;
    let mut day = 1;
    let mut date_only = true;
    if i < b.len() && b[i] == b'-' {
        i += 1;
        month = num(&mut i, 2)?;
        if i < b.len() && b[i] == b'-' {
            i += 1;
            day = num(&mut i, 2)?;
        }
    }
    let (mut h, mut mi, mut sec, mut ms) = (0, 0, 0, 0.0);
    let mut offset: Option<i64> = None;
    if i < b.len() && (b[i] == b'T' || b[i] == b't' || b[i] == b' ') {
        date_only = false;
        i += 1;
        h = num(&mut i, 2)?;
        if i >= b.len() || b[i] != b':' {
            return None;
        }
        i += 1;
        mi = num(&mut i, 2)?;
        if i < b.len() && b[i] == b':' {
            i += 1;
            sec = num(&mut i, 2)?;
            if i < b.len() && (b[i] == b'.' || b[i] == b',') {
                i += 1;
                let st = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i == st {
                    return None;
                }
                let frac = &s[st..i];
                let f: f64 = format!("0.{frac}").parse().ok()?;
                ms = (f * 1000.0).floor();
            }
        }
        if i < b.len() {
            if b[i] == b'Z' || b[i] == b'z' {
                offset = Some(0);
                i += 1;
            } else if b[i] == b'+' || b[i] == b'-' {
                let sign = if b[i] == b'-' { -1 } else { 1 };
                i += 1;
                let oh = num(&mut i, 2)?;
                if i < b.len() && b[i] == b':' {
                    i += 1;
                }
                let om = num(&mut i, 2)?;
                offset = Some(sign * (oh * 60 + om));
            }
        }
    }
    if i != b.len() {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 24 || mi > 59 || sec > 59 {
        return None;
    }
    let _ = date_only;
    let t = make_time(
        year as f64,
        (month - 1) as f64,
        day as f64,
        h as f64,
        mi as f64,
        sec as f64,
        ms,
    );
    Some(t - offset.unwrap_or(0) as f64 * 60_000.0)
}

fn parse_loose(s: &str) -> Option<f64> {
    // Tokens: words (month/day names), numbers, times hh:mm[:ss], AM/PM, GMT offsets.
    let mut year: Option<i64> = None;
    let mut month: Option<i64> = None;
    let mut day: Option<i64> = None;
    let (mut h, mut mi, mut sec) = (0i64, 0i64, 0i64);
    let mut pm: Option<bool> = None;
    let mut offset = 0i64;
    let cleaned: String = s.chars().map(|c| if c == ',' { ' ' } else { c }).collect();
    let toks: Vec<&str> = cleaned.split_whitespace().collect();
    let mut nums: Vec<i64> = vec![];
    for t in toks {
        let lower = t.to_ascii_lowercase();
        if t.contains(':') {
            let ps: Vec<&str> = t.split(':').collect();
            h = ps.first()?.parse().ok()?;
            mi = ps.get(1).map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
            sec = ps
                .get(2)
                .map(|x| x.split('.').next().unwrap_or("0").parse().unwrap_or(0))
                .unwrap_or(0);
            continue;
        }
        if lower == "am" || lower == "pm" {
            pm = Some(lower == "pm");
            continue;
        }
        if lower.starts_with("gmt") || lower.starts_with("utc") || lower == "z" {
            let rest = &t[3.min(t.len())..];
            if let Some(sign) = rest.chars().next() {
                if sign == '+' || sign == '-' {
                    let digits: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
                    if digits.len() >= 4 {
                        let hh: i64 = digits[..2].parse().ok()?;
                        let mm: i64 = digits[2..4].parse().ok()?;
                        offset = (hh * 60 + mm) * if sign == '-' { -1 } else { 1 };
                    }
                }
            }
            continue;
        }
        if t.starts_with('(') || t.ends_with(')') {
            continue;
        }
        if let Some(mi_) = MONTHS
            .iter()
            .position(|m| lower.starts_with(&m.to_ascii_lowercase()))
        {
            month = Some(mi_ as i64);
            continue;
        }
        if DAYS
            .iter()
            .any(|d| lower.starts_with(&d.to_ascii_lowercase()))
        {
            continue;
        }
        if t.contains('/') || (t.contains('-') && !t.starts_with('-')) {
            let sep = if t.contains('/') { '/' } else { '-' };
            let ps: Vec<i64> = t.split(sep).filter_map(|x| x.parse().ok()).collect();
            if ps.len() == 3 {
                if ps[0] > 31 {
                    year = Some(ps[0]);
                    month = Some(ps[1] - 1);
                    day = Some(ps[2]);
                } else {
                    month = Some(ps[0] - 1);
                    day = Some(ps[1]);
                    year = Some(ps[2]);
                }
                continue;
            }
            return None;
        }
        match t.parse::<i64>() {
            Ok(n) => nums.push(n),
            Err(_) => return None,
        }
    }
    for n in nums {
        if day.is_none() && (1..=31).contains(&n) && month.is_some() {
            day = Some(n);
        } else if year.is_none() {
            year = Some(if n < 50 {
                2000 + n
            } else if n < 100 {
                1900 + n
            } else {
                n
            });
        } else if day.is_none() {
            day = Some(n);
        } else {
            return None;
        }
    }
    let (year, month) = (year?, month?);
    let day = day.unwrap_or(1);
    if let Some(p) = pm {
        if p && h < 12 {
            h += 12;
        }
        if !p && h == 12 {
            h = 0;
        }
    }
    let t = make_time(
        year as f64,
        month as f64,
        day as f64,
        h as f64,
        mi as f64,
        sec as f64,
        0.0,
    );
    Some(t - offset as f64 * 60_000.0)
}

fn this_time(vm: &mut Vm, a: &Args) -> JsResult<f64> {
    if let Value::Obj(o) = &a.this {
        if let Kind::Date(t) = o.borrow().kind {
            return Ok(t);
        }
    }
    Err(vm.type_error("this is not a Date object."))
}

fn set_this_time(a: &Args, t: f64) {
    if let Value::Obj(o) = &a.this {
        if let Kind::Date(x) = &mut o.borrow_mut().kind {
            *x = t;
        }
    }
}

fn date_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        let t = vm.now_ms().floor();
        return Ok(Value::string(date_to_string(t)));
    };
    let t = match a.args.len() {
        0 => vm.now_ms().floor(),
        1 => {
            let v = a.arg(0);
            let tv = match &v {
                Value::Obj(o) if matches!(o.borrow().kind, Kind::Date(_)) => {
                    match o.borrow().kind {
                        Kind::Date(t) => Value::Num(t),
                        _ => unreachable!(),
                    }
                }
                _ => vm.to_primitive(&v, crate::conv::Hint::Default)?,
            };
            match tv {
                Value::Str(s) => parse_date(&s),
                other => time_clip(vm.to_number(&other)?),
            }
        }
        _ => {
            let mut n = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
            for (i, v) in a.args.iter().take(7).enumerate() {
                n[i] = vm.to_number(v)?;
            }
            let y = if n[0].is_finite() && (0.0..=99.0).contains(&n[0].trunc()) {
                1900.0 + n[0].trunc()
            } else {
                n[0]
            };
            make_time(y, n[1], n[2], n[3], n[4], n[5], n[6])
        }
    };
    let dp = vm.intr.date_proto.clone();
    let proto = vm.proto_from_ctor(&nt, &dp)?;
    Ok(Value::Obj(vm.obj_with(Some(proto), Kind::Date(t))))
}

fn date_now(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(vm.now_ms().floor()))
}

fn date_parse(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    Ok(Value::Num(parse_date(&s)))
}

fn date_utc(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut n = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (i, v) in a.args.iter().take(7).enumerate() {
        n[i] = vm.to_number(v)?;
    }
    let y = if n[0].is_finite() && (0.0..=99.0).contains(&n[0].trunc()) {
        1900.0 + n[0].trunc()
    } else {
        n[0]
    };
    Ok(Value::Num(make_time(y, n[1], n[2], n[3], n[4], n[5], n[6])))
}

macro_rules! getter {
    ($name:ident, $f:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
            let t = this_time(vm, a)?;
            if t.is_nan() {
                return Ok(Value::Num(f64::NAN));
            }
            let p = parts(t);
            let f: fn(&Parts) -> i64 = $f;
            Ok(Value::Num(f(&p) as f64))
        }
    };
}

getter!(get_full_year, |p| p.year);
getter!(get_month, |p| p.month);
getter!(get_date, |p| p.day);
getter!(get_day, |p| p.weekday);
getter!(get_hours, |p| p.hour);
getter!(get_minutes, |p| p.minute);
getter!(get_seconds, |p| p.second);
getter!(get_milliseconds, |p| p.ms);

fn get_time(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(this_time(vm, a)?))
}

fn get_timezone_offset(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    Ok(Value::Num(if t.is_nan() { f64::NAN } else { 0.0 }))
}

fn get_year(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    if t.is_nan() {
        return Ok(Value::Num(f64::NAN));
    }
    Ok(Value::Num((parts(t).year - 1900) as f64))
}

/// Generic setter: field index 0 year .. 6 ms; number of args consumed.
fn set_fields(vm: &mut Vm, a: &mut Args, first: usize, max: usize) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    let base = if t.is_nan() && first == 0 { 0.0 } else { t };
    if base.is_nan() {
        return Ok(Value::Num(f64::NAN));
    }
    let p = parts(base);
    let mut f = [
        p.year as f64,
        p.month as f64,
        p.day as f64,
        p.hour as f64,
        p.minute as f64,
        p.second as f64,
        p.ms as f64,
    ];
    for i in 0..max.min(a.args.len().max(1)) {
        f[first + i] = vm.to_number(&a.arg(i))?;
    }
    let nt = make_time(f[0], f[1], f[2], f[3], f[4], f[5], f[6]);
    set_this_time(a, nt);
    Ok(Value::Num(nt))
}

fn set_full_year(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 0, 3)
}
fn set_month(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 1, 2)
}
fn set_date(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 2, 1)
}
fn set_hours(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 3, 4)
}
fn set_minutes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 4, 3)
}
fn set_seconds(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 5, 2)
}
fn set_milliseconds(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_fields(vm, a, 6, 1)
}

fn set_time(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    this_time(vm, a)?;
    let n = num_arg(vm, a, 0)?;
    let t = time_clip(n);
    set_this_time(a, t);
    Ok(Value::Num(t))
}

fn to_iso_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    if t.is_nan() {
        return Err(vm.range_error("Invalid time value"));
    }
    Ok(Value::string(iso_string(t)))
}

fn to_json(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let tv = vm.to_primitive(&a.this, crate::conv::Hint::Number)?;
    if let Value::Num(n) = tv {
        if !n.is_finite() {
            return Ok(Value::Null);
        }
    }
    let f = vm.get_str(&a.this, "toISOString")?;
    vm.call(&f, a.this.clone(), vec![])
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    Ok(Value::string(date_to_string(t)))
}

fn to_date_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    if t.is_nan() {
        return Ok(Value::str("Invalid Date"));
    }
    let p = parts(t);
    Ok(Value::string(format!(
        "{} {} {:02} {}",
        DAYS[p.weekday as usize],
        MONTHS[p.month as usize],
        p.day,
        yr4(p.year)
    )))
}

fn to_time_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    if t.is_nan() {
        return Ok(Value::str("Invalid Date"));
    }
    let p = parts(t);
    Ok(Value::string(format!(
        "{:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        p.hour, p.minute, p.second
    )))
}

fn to_utc_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    if t.is_nan() {
        return Ok(Value::str("Invalid Date"));
    }
    let p = parts(t);
    Ok(Value::string(format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        DAYS[p.weekday as usize],
        p.day,
        MONTHS[p.month as usize],
        yr4(p.year),
        p.hour,
        p.minute,
        p.second
    )))
}

fn opt_str(vm: &mut Vm, opts: &Value, k: &str) -> JsResult<Option<String>> {
    if !matches!(opts, Value::Obj(_)) {
        return Ok(None);
    }
    let v = vm.get_str(opts, k)?;
    if v.is_undefined() {
        return Ok(None);
    }
    Ok(Some(vm.to_str(&v)?))
}

/// en-US formatting following Intl.DateTimeFormat option subsets.
pub fn locale_format(
    vm: &mut Vm,
    t: f64,
    opts: &Value,
    date: bool,
    time: bool,
) -> JsResult<String> {
    if t.is_nan() {
        return Ok("Invalid Date".into());
    }
    let p = parts(t);
    let weekday = opt_str(vm, opts, "weekday")?;
    let year = opt_str(vm, opts, "year")?;
    let month = opt_str(vm, opts, "month")?;
    let day = opt_str(vm, opts, "day")?;
    let hour = opt_str(vm, opts, "hour")?;
    let minute = opt_str(vm, opts, "minute")?;
    let second = opt_str(vm, opts, "second")?;
    let hour12 = if let Value::Obj(_) = opts {
        let v = vm.get_str(opts, "hour12")?;
        if v.is_undefined() {
            true
        } else {
            v.truthy()
        }
    } else {
        true
    };
    let date_style = opt_str(vm, opts, "dateStyle")?;
    let time_style = opt_str(vm, opts, "timeStyle")?;
    let any_field = weekday.is_some()
        || year.is_some()
        || month.is_some()
        || day.is_some()
        || hour.is_some()
        || minute.is_some()
        || second.is_some();
    let (use_date, use_time) = if date_style.is_some() || time_style.is_some() {
        (date_style.is_some(), time_style.is_some())
    } else if any_field {
        (false, false)
    } else {
        (date, time)
    };
    let fmt_time = |with_seconds: bool| -> String {
        if hour12 {
            let h = if p.hour % 12 == 0 { 12 } else { p.hour % 12 };
            let ampm = if p.hour < 12 { "AM" } else { "PM" };
            if with_seconds {
                format!("{h}:{:02}:{:02} {ampm}", p.minute, p.second)
            } else {
                format!("{h}:{:02} {ampm}", p.minute)
            }
        } else if with_seconds {
            format!("{:02}:{:02}:{:02}", p.hour, p.minute, p.second)
        } else {
            format!("{:02}:{:02}", p.hour, p.minute)
        }
    };
    if date_style.is_some() || time_style.is_some() || !any_field {
        let mut parts_out = vec![];
        if use_date {
            let s = match date_style.as_deref() {
                Some("full") => format!(
                    "{}, {} {}, {}",
                    DAYS_LONG[p.weekday as usize], MONTHS_LONG[p.month as usize], p.day, p.year
                ),
                Some("long") => format!("{} {}, {}", MONTHS_LONG[p.month as usize], p.day, p.year),
                Some("medium") => format!("{} {}, {}", MONTHS[p.month as usize], p.day, p.year),
                Some("short") => format!("{}/{}/{:02}", p.month + 1, p.day, p.year % 100),
                _ => format!("{}/{}/{}", p.month + 1, p.day, p.year),
            };
            parts_out.push(s);
        }
        if use_time {
            let s = match time_style.as_deref() {
                Some("short") => fmt_time(false),
                Some("full") => format!("{} Coordinated Universal Time", fmt_time(true)),
                Some("long") => format!("{} UTC", fmt_time(true)),
                _ => fmt_time(true),
            };
            parts_out.push(s);
        }
        let sep = if date_style
            .as_deref()
            .map(|d| d == "full" || d == "long")
            .unwrap_or(false)
            && use_time
        {
            " at "
        } else {
            ", "
        };
        return Ok(parts_out.join(sep));
    }
    // Field-based formatting.
    let mut date_part = String::new();
    let month_text = match month.as_deref() {
        Some("long") => Some(MONTHS_LONG[p.month as usize].to_string()),
        Some("short") => Some(MONTHS[p.month as usize].to_string()),
        Some("narrow") => Some(MONTHS_LONG[p.month as usize][..1].to_string()),
        _ => None,
    };
    let yr = match year.as_deref() {
        Some("2-digit") => Some(format!("{:02}", p.year % 100)),
        Some(_) => Some(p.year.to_string()),
        None => None,
    };
    let dy = match day.as_deref() {
        Some("2-digit") => Some(format!("{:02}", p.day)),
        Some(_) => Some(p.day.to_string()),
        None => None,
    };
    if let Some(mt) = &month_text {
        date_part.push_str(mt);
        if let Some(d) = &dy {
            date_part.push(' ');
            date_part.push_str(d);
        }
        if let Some(y) = &yr {
            if dy.is_some() {
                date_part.push(',');
            }
            date_part.push(' ');
            date_part.push_str(y);
        }
    } else {
        let mnum = match month.as_deref() {
            Some("2-digit") => Some(format!("{:02}", p.month + 1)),
            Some(_) => Some((p.month + 1).to_string()),
            None => None,
        };
        let comps: Vec<String> = [mnum, dy.clone(), yr.clone()]
            .into_iter()
            .flatten()
            .collect();
        date_part = comps.join("/");
    }
    let wd = match weekday.as_deref() {
        Some("long") => Some(DAYS_LONG[p.weekday as usize].to_string()),
        Some("short") => Some(DAYS[p.weekday as usize].to_string()),
        Some("narrow") => Some(DAYS_LONG[p.weekday as usize][..1].to_string()),
        _ => None,
    };
    let date_full = match (wd, date_part.is_empty()) {
        (Some(w), false) => format!("{w}, {date_part}"),
        (Some(w), true) => w,
        (None, _) => date_part,
    };
    let mut time_part = String::new();
    if hour.is_some() || minute.is_some() || second.is_some() {
        let h = if hour12 {
            if p.hour % 12 == 0 {
                12
            } else {
                p.hour % 12
            }
        } else {
            p.hour
        };
        let mut comps = vec![];
        if hour.is_some() {
            comps.push(if hour.as_deref() == Some("2-digit") || !hour12 {
                format!("{h:02}")
            } else {
                h.to_string()
            });
        }
        if minute.is_some() {
            comps.push(format!("{:02}", p.minute));
        }
        if second.is_some() {
            comps.push(format!("{:02}", p.second));
        }
        time_part = comps.join(":");
        if hour.is_some() && hour12 {
            time_part.push_str(if p.hour < 12 { " AM" } else { " PM" });
        }
    }
    Ok(match (date_full.is_empty(), time_part.is_empty()) {
        (false, false) => format!("{date_full}, {time_part}"),
        (false, true) => date_full,
        (true, false) => time_part,
        _ => String::new(),
    })
}

fn to_locale_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    let o = a.arg(1);
    Ok(Value::string(locale_format(vm, t, &o, true, true)?))
}
fn to_locale_date_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    let o = a.arg(1);
    Ok(Value::string(locale_format(vm, t, &o, true, false)?))
}
fn to_locale_time_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = this_time(vm, a)?;
    let o = a.arg(1);
    Ok(Value::string(locale_format(vm, t, &o, false, true)?))
}

fn to_primitive(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let hint = vm.to_str(&a.arg(0))?;
    let Value::Obj(o) = &a.this else {
        return Err(vm.type_error("Date.prototype[Symbol.toPrimitive] called on non-object"));
    };
    let order = if hint == "number" {
        ["valueOf", "toString"]
    } else {
        ["toString", "valueOf"]
    };
    for m in order {
        let f = vm.get_str(&Value::Obj(o.clone()), m)?;
        if f.is_callable() {
            let r = vm.call(&f, a.this.clone(), vec![])?;
            if !matches!(r, Value::Obj(_)) {
                return Ok(r);
            }
        }
    }
    Err(vm.type_error("Cannot convert object to primitive value"))
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.date_proto.clone();
    let ctor = vm.make_ctor("Date", 7, date_ctor, &proto);
    vm.method(&ctor, "now", 0, date_now);
    vm.method(&ctor, "parse", 1, date_parse);
    vm.method(&ctor, "UTC", 7, date_utc);
    vm.set_global("Date", Value::Obj(ctor));
    let fs: &[(&str, u32, NativeFn)] = &[
        ("getFullYear", 0, get_full_year),
        ("getMonth", 0, get_month),
        ("getDate", 0, get_date),
        ("getDay", 0, get_day),
        ("getHours", 0, get_hours),
        ("getMinutes", 0, get_minutes),
        ("getSeconds", 0, get_seconds),
        ("getMilliseconds", 0, get_milliseconds),
        ("getUTCFullYear", 0, get_full_year),
        ("getUTCMonth", 0, get_month),
        ("getUTCDate", 0, get_date),
        ("getUTCDay", 0, get_day),
        ("getUTCHours", 0, get_hours),
        ("getUTCMinutes", 0, get_minutes),
        ("getUTCSeconds", 0, get_seconds),
        ("getUTCMilliseconds", 0, get_milliseconds),
        ("getTime", 0, get_time),
        ("valueOf", 0, get_time),
        ("getYear", 0, get_year),
        ("getTimezoneOffset", 0, get_timezone_offset),
        ("setFullYear", 3, set_full_year),
        ("setMonth", 2, set_month),
        ("setDate", 1, set_date),
        ("setHours", 4, set_hours),
        ("setMinutes", 3, set_minutes),
        ("setSeconds", 2, set_seconds),
        ("setMilliseconds", 1, set_milliseconds),
        ("setUTCFullYear", 3, set_full_year),
        ("setUTCMonth", 2, set_month),
        ("setUTCDate", 1, set_date),
        ("setUTCHours", 4, set_hours),
        ("setUTCMinutes", 3, set_minutes),
        ("setUTCSeconds", 2, set_seconds),
        ("setUTCMilliseconds", 1, set_milliseconds),
        ("setTime", 1, set_time),
        ("toISOString", 0, to_iso_string),
        ("toJSON", 1, to_json),
        ("toString", 0, to_string),
        ("toDateString", 0, to_date_string),
        ("toTimeString", 0, to_time_string),
        ("toUTCString", 0, to_utc_string),
        ("toGMTString", 0, to_utc_string),
        ("toLocaleString", 0, to_locale_string),
        ("toLocaleDateString", 0, to_locale_date_string),
        ("toLocaleTimeString", 0, to_locale_time_string),
    ];
    for (n, l, f) in fs {
        vm.method(&proto, n, *l, *f);
    }
    let tp = vm.syms.to_primitive.clone();
    let f = vm.native_fn("[Symbol.toPrimitive]", 1, to_primitive);
    proto.set_sym(&tp, Value::Obj(f), CONFIGURABLE);
}
