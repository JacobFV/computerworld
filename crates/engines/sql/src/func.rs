//! Scalar SQL functions. Aggregates live in `exec`, which owns grouping.
use crate::eval::Ctx;
use crate::value::{compare, format_real, hex, unhex, Collation, Value};
use crate::SqlError;
use cw_determinism::math;
use std::cmp::Ordering;

fn argc(
    name: &str,
    args: &[Value],
    range: std::ops::RangeInclusive<usize>,
) -> Result<(), SqlError> {
    if range.contains(&args.len()) {
        Ok(())
    } else {
        Err(SqlError::new(format!(
            "wrong number of arguments to function {name}()"
        )))
    }
}
fn text(v: &Value) -> String {
    v.to_text()
}
fn num(v: &Value) -> Option<f64> {
    v.to_f64()
}
fn real_or_null(r: f64) -> Value {
    if r.is_nan() {
        Value::Null
    } else {
        Value::Real(r)
    }
}
/// One-argument math over a real; NULL in, NULL out.
fn math1(args: &[Value], f: impl Fn(f64) -> f64) -> Value {
    match num(&args[0]) {
        Some(x) if !args[0].is_null() => real_or_null(f(x)),
        _ => Value::Null,
    }
}

/// Every scalar function this engine implements, for `.help` and completion.
pub const SCALAR_FUNCTIONS: &[&str] = &[
    "abs",
    "acos",
    "asin",
    "atan",
    "atan2",
    "ceil",
    "ceiling",
    "changes",
    "char",
    "coalesce",
    "cos",
    "cosh",
    "date",
    "datetime",
    "degrees",
    "exp",
    "floor",
    "format",
    "glob",
    "hex",
    "ifnull",
    "iif",
    "instr",
    "julianday",
    "last_insert_rowid",
    "length",
    "like",
    "ln",
    "log",
    "log10",
    "log2",
    "lower",
    "ltrim",
    "max",
    "min",
    "mod",
    "nullif",
    "octet_length",
    "pi",
    "pow",
    "power",
    "printf",
    "quote",
    "radians",
    "random",
    "randomblob",
    "replace",
    "round",
    "rtrim",
    "sign",
    "sin",
    "sinh",
    "soundex",
    "sqlite_version",
    "sqrt",
    "strftime",
    "substr",
    "substring",
    "tan",
    "tanh",
    "time",
    "total_changes",
    "trim",
    "trunc",
    "typeof",
    "unhex",
    "unicode",
    "unixepoch",
    "upper",
    "zeroblob",
];

pub fn call(
    ctx: &Ctx,
    name: &str,
    args: &[Value],
    collation: Collation,
) -> Result<Value, SqlError> {
    let now = ctx.env.now_us;
    Ok(match name {
        "abs" => {
            argc(name, args, 1..=1)?;
            match args[0].to_number() {
                Value::Integer(i) => Value::Integer(
                    i.checked_abs()
                        .ok_or_else(|| SqlError::new("integer overflow"))?,
                ),
                Value::Real(r) => Value::Real(r.abs()),
                _ => Value::Null,
            }
        }
        "changes" => Value::Integer(ctx.env.changes.get()),
        "total_changes" => Value::Integer(ctx.env.total_changes.get()),
        "last_insert_rowid" => Value::Integer(ctx.env.last_insert_rowid.get()),
        "char" => Value::Text(
            args.iter()
                .filter_map(|a| a.to_i64())
                .map(|c| char::from_u32(c as u32).unwrap_or('\u{fffd}'))
                .collect(),
        ),
        "glob" => {
            argc(name, args, 2..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            Value::Integer(i64::from(crate::eval::glob(
                &text(&args[0]),
                &text(&args[1]),
            )))
        }
        "like" => {
            argc(name, args, 2..=3)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let esc = args.get(2).and_then(|e| text(e).chars().next());
            Value::Integer(i64::from(crate::eval::like(
                &text(&args[0]),
                &text(&args[1]),
                esc,
            )))
        }
        "hex" => {
            argc(name, args, 1..=1)?;
            match &args[0] {
                Value::Blob(b) => Value::Text(hex(b)),
                Value::Null => Value::Text(String::new()),
                v => Value::Text(hex(text(v).as_bytes())),
            }
        }
        "unhex" => {
            argc(name, args, 1..=2)?;
            if args[0].is_null() {
                return Ok(Value::Null);
            }
            let ignore = args.get(1).map(text).unwrap_or_default();
            let cleaned: String = text(&args[0])
                .chars()
                .filter(|c| !ignore.contains(*c))
                .collect();
            unhex(&cleaned).map_or(Value::Null, Value::Blob)
        }
        "instr" => {
            argc(name, args, 2..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            match (&args[0], &args[1]) {
                (Value::Blob(h), Value::Blob(n)) => Value::Integer(
                    h.windows(n.len().max(1))
                        .position(|w| w == n.as_slice())
                        .map_or(0, |p| p as i64 + 1),
                ),
                (h, n) => {
                    let (h, n) = (text(h), text(n));
                    Value::Integer(h.find(&n).map_or(0, |b| h[..b].chars().count() as i64 + 1))
                }
            }
        }
        "length" => {
            argc(name, args, 1..=1)?;
            match &args[0] {
                Value::Null => Value::Null,
                Value::Blob(b) => Value::Integer(b.len() as i64),
                v => Value::Integer(text(v).chars().count() as i64),
            }
        }
        "octet_length" => {
            argc(name, args, 1..=1)?;
            match &args[0] {
                Value::Null => Value::Null,
                Value::Blob(b) => Value::Integer(b.len() as i64),
                v => Value::Integer(text(v).len() as i64),
            }
        }
        "lower" | "upper" => {
            argc(name, args, 1..=1)?;
            if args[0].is_null() {
                return Ok(Value::Null);
            }
            // SQLite folds ASCII only unless built with ICU.
            let t = text(&args[0]);
            Value::Text(if name == "lower" {
                t.to_ascii_lowercase()
            } else {
                t.to_ascii_uppercase()
            })
        }
        "ltrim" | "rtrim" | "trim" => {
            argc(name, args, 1..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let t = text(&args[0]);
            let set: Vec<char> = match args.get(1) {
                Some(s) => text(s).chars().collect(),
                None => vec![' '],
            };
            let p = |c: char| set.contains(&c);
            Value::Text(match name {
                "ltrim" => t.trim_start_matches(p).to_owned(),
                "rtrim" => t.trim_end_matches(p).to_owned(),
                _ => t.trim_matches(p).to_owned(),
            })
        }
        "max" | "min" => {
            if args.len() < 2 {
                return Err(SqlError::new(format!(
                    "wrong number of arguments to function {name}()"
                )));
            }
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let want = if name == "max" {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            let mut best = args[0].clone();
            for a in &args[1..] {
                if compare(a, &best, collation) == want {
                    best = a.clone();
                }
            }
            best
        }
        "nullif" => {
            argc(name, args, 2..=2)?;
            if compare(&args[0], &args[1], collation) == Ordering::Equal && !args[0].is_null() {
                Value::Null
            } else {
                args[0].clone()
            }
        }
        "printf" | "format" => {
            if args.is_empty() {
                return Ok(Value::Null);
            }
            if args[0].is_null() {
                return Ok(Value::Null);
            }
            Value::Text(printf(&text(&args[0]), &args[1..]))
        }
        "quote" => {
            argc(name, args, 1..=1)?;
            Value::Text(args[0].quoted())
        }
        "random" => {
            // A SplitMix64 stream kept with the connection: every replay draws the
            // same numbers, which is the only honest "random" a simulation can have.
            let mut s = ctx.env.rng.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
            ctx.env.rng.set(s);
            s = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            s = (s ^ (s >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            Value::Integer((s ^ (s >> 31)) as i64)
        }
        "randomblob" => {
            argc(name, args, 1..=1)?;
            let n = args[0].to_i64().unwrap_or(1).clamp(1, 1_000_000) as usize;
            let mut out = Vec::with_capacity(n);
            while out.len() < n {
                if let Value::Integer(r) = call(ctx, "random", &[], collation)? {
                    out.extend_from_slice(&r.to_le_bytes());
                }
            }
            out.truncate(n);
            Value::Blob(out)
        }
        "zeroblob" => {
            argc(name, args, 1..=1)?;
            let n = args[0].to_i64().unwrap_or(0).clamp(0, 1_000_000_000) as usize;
            if n > 16 << 20 {
                return Err(SqlError::new("string or blob too big"));
            }
            Value::Blob(vec![0; n])
        }
        "replace" => {
            argc(name, args, 3..=3)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let (t, from, to) = (text(&args[0]), text(&args[1]), text(&args[2]));
            if from.is_empty() {
                Value::Text(t)
            } else {
                Value::Text(t.replace(&from, &to))
            }
        }
        "round" => {
            argc(name, args, 1..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let x = num(&args[0]).unwrap_or(0.0);
            let n = args
                .get(1)
                .and_then(Value::to_i64)
                .unwrap_or(0)
                .clamp(0, 30) as usize;
            Value::Real(round(x, n))
        }
        "sign" => {
            argc(name, args, 1..=1)?;
            match args[0].to_number() {
                Value::Integer(i) if !matches!(args[0], Value::Text(ref t) if crate::value::exact_number(t).is_none()) => {
                    Value::Integer(i.signum())
                }
                Value::Real(r) if !matches!(args[0], Value::Text(ref t) if crate::value::exact_number(t).is_none()) => {
                    Value::Integer(if r > 0.0 {
                        1
                    } else if r < 0.0 {
                        -1
                    } else {
                        0
                    })
                }
                _ => Value::Null,
            }
        }
        "soundex" => {
            argc(name, args, 1..=1)?;
            Value::Text(soundex(&text(&args[0])))
        }
        "sqlite_version" => Value::Text(crate::SQLITE_VERSION.into()),
        "substr" | "substring" => {
            argc(name, args, 2..=3)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let start = args[1].to_i64().unwrap_or(0);
            let len = args.get(2).map(|l| l.to_i64().unwrap_or(0));
            match &args[0] {
                Value::Blob(b) => Value::Blob(substr(b, start, len)),
                v => {
                    let chars: Vec<char> = text(v).chars().collect();
                    Value::Text(substr(&chars, start, len).into_iter().collect())
                }
            }
        }
        "typeof" => {
            argc(name, args, 1..=1)?;
            Value::Text(args[0].type_name().into())
        }
        "unicode" => {
            argc(name, args, 1..=1)?;
            if args[0].is_null() {
                return Ok(Value::Null);
            }
            text(&args[0])
                .chars()
                .next()
                .map_or(Value::Null, |c| Value::Integer(c as i64))
        }
        "date" => crate::datetime::date(args, now),
        "time" => crate::datetime::time(args, now),
        "datetime" => crate::datetime::datetime(args, now),
        "julianday" => crate::datetime::julianday(args, now),
        "unixepoch" => crate::datetime::unixepoch(args, now),
        "strftime" => crate::datetime::strftime(args, now),
        "current_date" => crate::datetime::date(&[Value::Text("now".into())], now),
        "current_time" => crate::datetime::time(&[Value::Text("now".into())], now),
        "current_timestamp" => crate::datetime::datetime(&[Value::Text("now".into())], now),
        // Math functions (SQLITE_ENABLE_MATH_FUNCTIONS), computed deterministically.
        "ceil" | "ceiling" | "floor" | "trunc" => {
            argc(name, args, 1..=1)?;
            match args[0].to_number() {
                Value::Integer(i) if !matches!(args[0], Value::Text(_)) => Value::Integer(i),
                Value::Null => Value::Null,
                n => {
                    let x = n.to_f64().unwrap_or(0.0);
                    Value::Real(match name {
                        "floor" => x.floor(),
                        "trunc" => x.trunc(),
                        _ => x.ceil(),
                    })
                }
            }
        }
        "exp" => {
            argc(name, args, 1..=1)?;
            math1(args, math::exp)
        }
        "ln" => {
            argc(name, args, 1..=1)?;
            math1(args, |x| if x > 0.0 { math::ln(x) } else { f64::NAN })
        }
        "log10" => {
            argc(name, args, 1..=1)?;
            math1(args, |x| if x > 0.0 { math::log10(x) } else { f64::NAN })
        }
        "log2" => {
            argc(name, args, 1..=1)?;
            math1(args, |x| if x > 0.0 { math::log2(x) } else { f64::NAN })
        }
        "log" => {
            argc(name, args, 1..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            if args.len() == 1 {
                math1(args, |x| if x > 0.0 { math::log10(x) } else { f64::NAN })
            } else {
                let (b, x) = (num(&args[0]).unwrap_or(0.0), num(&args[1]).unwrap_or(0.0));
                if b <= 0.0 || b == 1.0 || x <= 0.0 {
                    Value::Null
                } else if b == 2.0 {
                    Value::Real(math::log2(x))
                } else if b == 10.0 {
                    Value::Real(math::log10(x))
                } else {
                    Value::Real(math::ln(x) / math::ln(b))
                }
            }
        }
        "pow" | "power" => {
            argc(name, args, 2..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            real_or_null(math::pow(
                num(&args[0]).unwrap_or(0.0),
                num(&args[1]).unwrap_or(0.0),
            ))
        }
        "sqrt" => {
            argc(name, args, 1..=1)?;
            math1(args, f64::sqrt)
        }
        "mod" => {
            argc(name, args, 2..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let (a, b) = (num(&args[0]).unwrap_or(0.0), num(&args[1]).unwrap_or(0.0));
            if b == 0.0 {
                Value::Null
            } else {
                Value::Real(a % b)
            }
        }
        "pi" => Value::Real(std::f64::consts::PI),
        "sin" => math1(args, math::sin),
        "cos" => math1(args, math::cos),
        "tan" => math1(args, math::tan),
        "asin" => math1(args, math::asin),
        "acos" => math1(args, math::acos),
        "atan" => math1(args, math::atan),
        "atan2" => {
            argc(name, args, 2..=2)?;
            if args.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            Value::Real(math::atan2(
                num(&args[0]).unwrap_or(0.0),
                num(&args[1]).unwrap_or(0.0),
            ))
        }
        "sinh" => math1(args, math::sinh),
        "cosh" => math1(args, math::cosh),
        "tanh" => math1(args, math::tanh),
        "degrees" => math1(args, |x| x * (180.0 / std::f64::consts::PI)),
        "radians" => math1(args, |x| x * (std::f64::consts::PI / 180.0)),
        "sqlite_source_id" => {
            Value::Text(format!("computerworld cw-sql {}", crate::SQLITE_VERSION))
        }
        _ => {
            return Err(SqlError::new(format!("no such function: {name}")));
        }
    })
}
fn substr<T: Clone>(items: &[T], start: i64, len: Option<i64>) -> Vec<T> {
    let n = items.len() as i64;
    // SQLite's arithmetic: positions are 1-based, 0 counts as "before the first", and a
    // negative start counts from the end.
    let (mut from, mut count) = (start, len.unwrap_or(i64::MAX / 4));
    if from < 0 {
        from += n + 1;
        if from < 1 {
            count += from - 1;
            from = 1;
        }
    } else if from == 0 {
        count -= 1;
        from = 1;
    }
    if count < 0 {
        // A negative length takes the characters before `from`.
        let end = from - 1;
        let begin = (end + count).max(0);
        return items[begin.clamp(0, n) as usize..end.clamp(0, n) as usize].to_vec();
    }
    let begin = (from - 1).clamp(0, n);
    let end = begin.saturating_add(count).clamp(0, n);
    items[begin as usize..end as usize].to_vec()
}
/// SQLite's `round()`: half away from zero on the decimal digits of the value.
pub fn round(x: f64, n: usize) -> f64 {
    if n == 0 && x.abs() < 4_503_599_627_370_496.0 {
        let r = if x < 0.0 { x - 0.5 } else { x + 0.5 };
        return r.trunc();
    }
    fixed(x, n).parse().unwrap_or(x)
}
/// `x` with exactly `decimals` fraction digits, rounding half away from zero on the
/// value's exact decimal expansion (as SQLite's printf does).
pub fn fixed(x: f64, decimals: usize) -> String {
    if !x.is_finite() {
        return format_real(x);
    }
    // Exact digits well beyond the rounding position, then a decimal half-up round.
    let sci = format!("{:.40e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mant
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // Value = 0.d1d2d3... × 10^(exp+1)
    let point = exp + 1;
    let keep = point + decimals as i64;
    let mut kept: Vec<u8> = if keep <= 0 {
        vec![]
    } else {
        digits.iter().copied().take(keep as usize).collect()
    };
    while (kept.len() as i64) < keep {
        kept.push(0);
    }
    let round_digit = if keep < 0 {
        0
    } else {
        digits.get(keep as usize).copied().unwrap_or(0)
    };
    let mut int_part_len = point.max(0) as usize;
    if round_digit >= 5 {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, 1);
                int_part_len += 1;
                break;
            }
            i -= 1;
            if kept[i] == 9 {
                kept[i] = 0;
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    // Pad leading zeros for values below one.
    let mut all = kept;
    let needed = int_part_len + decimals;
    while all.len() < needed {
        all.insert(0, 0);
    }
    let split = all.len() - decimals;
    let mut out = String::new();
    if x < 0.0 && all.iter().any(|d| *d != 0) {
        out.push('-');
    }
    let int: String = all[..split].iter().map(|d| (b'0' + d) as char).collect();
    out.push_str(if int.is_empty() {
        "0"
    } else {
        int.trim_start_matches('0')
    });
    if out.ends_with('-') || out.is_empty() {
        out.push('0');
    }
    if decimals > 0 {
        out.push('.');
        out.extend(all[split..].iter().map(|d| (b'0' + d) as char));
    }
    out
}
fn soundex(s: &str) -> String {
    let code = |c: char| match c.to_ascii_uppercase() {
        'B' | 'F' | 'P' | 'V' => Some('1'),
        'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => Some('2'),
        'D' | 'T' => Some('3'),
        'L' => Some('4'),
        'M' | 'N' => Some('5'),
        'R' => Some('6'),
        _ => None,
    };
    let mut chars = s.chars().skip_while(|c| !c.is_ascii_alphabetic());
    let Some(first) = chars.next() else {
        return "?000".into();
    };
    let mut out = String::from(first.to_ascii_uppercase());
    let mut last = code(first);
    for c in chars {
        if !c.is_ascii_alphabetic() {
            continue;
        }
        let k = code(c);
        if let Some(k) = k {
            if Some(k) != last {
                out.push(k);
                if out.len() == 4 {
                    break;
                }
            }
        }
        if !matches!(c.to_ascii_uppercase(), 'H' | 'W') {
            last = k;
        }
    }
    while out.len() < 4 {
        out.push('0');
    }
    out
}

/// SQLite's printf: `%d %i %u %f %e %E %g %G %x %X %o %c %s %z %q %Q %w %%` with
/// flags `- + space 0 # ,`, width and precision (either may be `*`).
pub fn printf(format: &str, args: &[Value]) -> String {
    let mut out = String::new();
    let mut next = 0;
    let mut take = || {
        let v = args.get(next).cloned().unwrap_or(Value::Null);
        next += 1;
        v
    };
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '%' {
            out.push(c);
            continue;
        }
        let (mut left, mut plus, mut space, mut zero, mut alt, mut comma) =
            (false, false, false, false, false, false);
        while i < chars.len() {
            match chars[i] {
                '-' => left = true,
                '+' => plus = true,
                ' ' => space = true,
                '0' => zero = true,
                '#' => alt = true,
                ',' => comma = true,
                '!' => {}
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        if i < chars.len() && chars[i] == '*' {
            let w = take().to_i64().unwrap_or(0);
            if w < 0 {
                left = true;
            }
            width = w.unsigned_abs() as usize;
            i += 1;
        } else {
            while i < chars.len() && chars[i].is_ascii_digit() {
                width = width * 10 + chars[i].to_digit(10).unwrap_or(0) as usize;
                i += 1;
            }
        }
        let mut precision: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            if i < chars.len() && chars[i] == '*' {
                precision = Some(take().to_i64().unwrap_or(0).max(0) as usize);
                i += 1;
            } else {
                let mut p = 0;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    p = p * 10 + chars[i].to_digit(10).unwrap_or(0) as usize;
                    i += 1;
                }
                precision = Some(p);
            }
        }
        while i < chars.len() && matches!(chars[i], 'l' | 'h') {
            i += 1;
        }
        let Some(&conv) = chars.get(i) else {
            break;
        };
        i += 1;
        let sign = |neg: bool| {
            if neg {
                "-"
            } else if plus {
                "+"
            } else if space {
                " "
            } else {
                ""
            }
        };
        let body: String = match conv {
            '%' => "%".into(),
            'd' | 'i' | 'u' => {
                let v = take();
                let n = if v.is_null() {
                    0
                } else {
                    v.to_i64().unwrap_or(0)
                };
                let mut digits = n.unsigned_abs().to_string();
                if let Some(p) = precision {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                if comma {
                    digits = group_thousands(&digits);
                }
                pad_number(sign(n < 0), &digits, width, left, zero)
            }
            'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                let v = take();
                let x = if v.is_null() {
                    0.0
                } else {
                    v.to_f64().unwrap_or(0.0)
                };
                let p = precision.unwrap_or(6);
                let mut digits = match conv {
                    'f' | 'F' => fixed(x.abs(), p),
                    'e' | 'E' => exponent(x.abs(), p, conv == 'E'),
                    _ => general(x.abs(), p.max(1), alt, conv == 'G'),
                };
                if comma && matches!(conv, 'f' | 'F') {
                    let (int, frac) = digits.split_once('.').unwrap_or((&digits, ""));
                    digits = if frac.is_empty() {
                        group_thousands(int)
                    } else {
                        format!("{}.{frac}", group_thousands(int))
                    };
                }
                if x.is_infinite() {
                    digits = "Inf".into();
                }
                pad_number(
                    sign(x < 0.0 || (x == 0.0 && x.is_sign_negative() && false)),
                    &digits,
                    width,
                    left,
                    zero && !x.is_infinite(),
                )
            }
            'x' | 'X' | 'o' => {
                let n = take().to_i64().unwrap_or(0) as u64;
                let mut digits = match conv {
                    'x' => format!("{n:x}"),
                    'X' => format!("{n:X}"),
                    _ => format!("{n:o}"),
                };
                if let Some(p) = precision {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                if alt && n != 0 {
                    digits.insert_str(
                        0,
                        match conv {
                            'x' => "0x",
                            'X' => "0X",
                            _ => "0",
                        },
                    );
                }
                pad_number("", &digits, width, left, zero)
            }
            'c' => {
                let v = take();
                let ch = v
                    .to_text()
                    .chars()
                    .next()
                    .map(String::from)
                    .unwrap_or_default();
                let n = precision.unwrap_or(1).max(1);
                pad_text(&ch.repeat(n), width, left)
            }
            's' | 'z' => {
                let v = take();
                let mut t = v.to_text();
                if let Some(p) = precision {
                    t = t.chars().take(p).collect();
                }
                pad_text(&t, width, left)
            }
            'q' | 'Q' | 'w' => {
                let v = take();
                let quote = if conv == 'w' { '"' } else { '\'' };
                let t = if v.is_null() {
                    if conv == 'Q' {
                        "NULL".into()
                    } else {
                        "(NULL)".into()
                    }
                } else {
                    let escaped = v.to_text().replace(quote, &format!("{quote}{quote}"));
                    if conv == 'Q' {
                        format!("'{escaped}'")
                    } else {
                        escaped
                    }
                };
                pad_text(&t, width, left)
            }
            other => {
                // An unknown conversion ends the output, as SQLite's printf does.
                let _ = other;
                break;
            }
        };
        out.push_str(&body);
    }
    out
}
fn group_thousands(int: &str) -> String {
    let b = int.as_bytes();
    let mut out = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}
fn pad_number(sign: &str, digits: &str, width: usize, left: bool, zero: bool) -> String {
    let len = sign.len() + digits.chars().count();
    if len >= width {
        return format!("{sign}{digits}");
    }
    let fill = width - len;
    if left {
        format!("{sign}{digits}{}", " ".repeat(fill))
    } else if zero {
        format!("{sign}{}{digits}", "0".repeat(fill))
    } else {
        format!("{}{sign}{digits}", " ".repeat(fill))
    }
}
fn pad_text(t: &str, width: usize, left: bool) -> String {
    let len = t.chars().count();
    if len >= width {
        return t.into();
    }
    if left {
        format!("{t}{}", " ".repeat(width - len))
    } else {
        format!("{}{t}", " ".repeat(width - len))
    }
}
fn exponent(x: f64, p: usize, upper: bool) -> String {
    if x == 0.0 {
        let zeros = if p > 0 {
            format!(".{}", "0".repeat(p))
        } else {
            String::new()
        };
        return format!("0{zeros}{}", if upper { "E+00" } else { "e+00" });
    }
    // Scale to one integer digit with the decimal rounding `fixed` uses.
    let mut e = math::log10(x).floor() as i32;
    let mut m = x / math::pow(10.0, f64::from(e));
    if m >= 10.0 {
        m /= 10.0;
        e += 1;
    }
    let mut mant = fixed(m, p);
    if mant.starts_with("10") {
        e += 1;
        mant = fixed(m / 10.0, p);
    }
    format!(
        "{mant}{}{}{:02}",
        if upper { 'E' } else { 'e' },
        if e < 0 { '-' } else { '+' },
        e.abs()
    )
}
fn general(x: f64, p: usize, alt: bool, upper: bool) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let e = {
        let s = exponent(x, p - 1, false);
        s.rsplit_once('e')
            .and_then(|(_, e)| e.parse::<i32>().ok())
            .unwrap_or(0)
    };
    let s = if e < -4 || e >= p as i32 {
        let s = exponent(x, p - 1, upper);
        if alt {
            s
        } else {
            let (m, ex) = s.split_at(s.find(['e', 'E']).unwrap_or(s.len()));
            let m = if m.contains('.') {
                m.trim_end_matches('0').trim_end_matches('.')
            } else {
                m
            };
            format!("{m}{ex}")
        }
    } else {
        let decimals = (p as i32 - 1 - e).max(0) as usize;
        let s = fixed(x, decimals);
        if alt || !s.contains('.') {
            s
        } else {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
    };
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(s: &str) -> Value {
        Value::Text(s.into())
    }
    #[test]
    fn printf_matches_sqlite() {
        let args = [
            Value::Integer(42),
            Value::Integer(42),
            Value::Integer(42),
            Value::Integer(42),
            t("abcdef"),
            Value::Real(3.25159265),
            Value::Real(1234.5),
            Value::Real(0.0001),
            Value::Real(1e20),
            t("xyz"),
            t("it's"),
            t("a"),
            Value::Integer(255),
            Value::Integer(8),
            Value::Null,
        ];
        assert_eq!(
            printf(
                "%d|%5d|%-5d|%05d|%.3s|%10.4f|%e|%g|%g|%c|%q|%Q|%x|%o|%%|%s",
                &args
            ),
            "42|   42|42   |00042|abc|    3.2516|1.234500e+03|0.0001|1e+20|x|it''s|'a'|ff|10|%|"
        );
        assert_eq!(printf("%,d", &[Value::Integer(1234567)]), "1,234,567");
        assert_eq!(printf("%.2f", &[Value::Real(0.125)]), "0.13");
    }
    #[test]
    fn rounding_follows_decimal_digits() {
        assert_eq!(round(2.675, 2), 2.67);
        assert_eq!(round(-0.5, 0), -1.0);
        assert_eq!(round(2.5, 0), 3.0);
        assert_eq!(round(1.005, 2), 1.0);
        assert_eq!(fixed(0.05, 1), "0.1");
        assert_eq!(fixed(-0.001, 2), "0.00");
        assert_eq!(fixed(999.996, 2), "1000.00");
        assert_eq!(soundex("Robert"), "R163");
        assert_eq!(substr(&['a', 'b', 'c'], 0, None), vec!['a', 'b', 'c']);
        assert_eq!(
            substr(&['a', 'b', 'c', 'd', 'e', 'f'], -2, None),
            vec!['e', 'f']
        );
        assert_eq!(
            substr(&['h', 'e', 'l', 'l', 'o'], -3, Some(2)),
            vec!['l', 'l']
        );
    }
}
