//! Text formatting with CPython's exact rules: float repr, `format()` specs,
//! `%` formatting, `str.format`, reprs of str/bytes, and name suggestions.
use crate::bigint::BigInt;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

// ---------- float repr ----------

/// Shortest round-tripping digits and decimal exponent of a finite, nonzero float.
fn shortest_digits(f: f64) -> (String, i32) {
    let s = format!("{:e}", f.abs());
    let (mant, exp) = s.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    (digits, exp.parse().unwrap())
}

pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let (digits, exp) = shortest_digits(f);
    let sign = if f < 0.0 { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let s = if exp >= 0 {
            let e = exp as usize;
            if digits.len() > e + 1 {
                format!("{}.{}", &digits[..e + 1], &digits[e + 1..])
            } else {
                format!("{}{}.0", digits, "0".repeat(e + 1 - digits.len()))
            }
        } else {
            format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
        };
        format!("{sign}{s}")
    } else {
        let mant = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        format!(
            "{sign}{mant}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

pub fn complex_repr(r: f64, i: f64) -> String {
    let fmt = |x: f64| {
        let s = float_repr(x);
        s.strip_suffix(".0").map(|t| t.to_string()).unwrap_or(s)
    };
    if r == 0.0 && !r.is_sign_negative() {
        return format!("{}j", fmt(i));
    }
    let im = fmt(i);
    let sign = if im.starts_with('-') || (i.is_nan()) {
        ""
    } else {
        "+"
    };
    format!("({}{}{}j)", fmt(r), sign, im)
}

// ---------- string reprs ----------

fn is_printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    if c.is_control() || c.is_whitespace() {
        return false;
    }
    let u = c as u32;
    !matches!(u,
        0xad | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x180e | 0x200b..=0x200f
        | 0x202a..=0x202e | 0x2060..=0x206f | 0xfeff | 0xfff9..=0xfffb
        | 0xe000..=0xf8ff | 0xd800..=0xdfff | 0xf0000..=0x10ffff | 0xe0000..=0xe007f
        | 0xfdd0..=0xfdef | 0xfffe | 0xffff)
}

pub fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c if (c as u32) < 0x7f => out.push(c),
            c if !is_printable(c) => {
                let u = c as u32;
                if u <= 0xff {
                    out.push_str(&format!("\\x{u:02x}"));
                } else if u <= 0xffff {
                    out.push_str(&format!("\\u{u:04x}"));
                } else {
                    out.push_str(&format!("\\U{u:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

pub fn ascii_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let u = c as u32;
        if u < 0x80 {
            out.push(c);
        } else if u <= 0xff {
            out.push_str(&format!("\\x{u:02x}"));
        } else if u <= 0xffff {
            out.push_str(&format!("\\u{u:04x}"));
        } else {
            out.push_str(&format!("\\U{u:08x}"));
        }
    }
    out
}

pub fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from("b");
    out.push(quote);
    for &c in b {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            c if c as char == quote => {
                out.push('\\');
                out.push(c as char);
            }
            0x20..=0x7e => out.push(c as char),
            c => out.push_str(&format!("\\x{c:02x}")),
        }
    }
    out.push(quote);
    out
}

// ---------- suggestions ("Did you mean") ----------

const MOVE_COST: usize = 2;
const CASE_COST: usize = 1;

fn substitution_cost(a: u8, b: u8) -> usize {
    if (a & 31) != (b & 31) {
        return MOVE_COST;
    }
    if a == b {
        return 0;
    }
    if a.eq_ignore_ascii_case(&b) {
        return CASE_COST;
    }
    MOVE_COST
}

fn levenshtein(a: &[u8], b: &[u8], max_cost: usize) -> usize {
    let (mut a, mut b) = (a, b);
    while !a.is_empty() && !b.is_empty() && a[0] == b[0] {
        a = &a[1..];
        b = &b[1..];
    }
    while !a.is_empty() && !b.is_empty() && a[a.len() - 1] == b[b.len() - 1] {
        a = &a[..a.len() - 1];
        b = &b[..b.len() - 1];
    }
    if a.is_empty() || b.is_empty() {
        return (a.len() + b.len()) * MOVE_COST;
    }
    if a.len() > 40 || b.len() > 40 {
        return max_cost + 1;
    }
    if b.len() < a.len() {
        std::mem::swap(&mut a, &mut b);
    }
    if (b.len() - a.len()) * MOVE_COST > max_cost {
        return max_cost + 1;
    }
    let mut buffer: Vec<usize> = (1..=a.len()).map(|i| i * MOVE_COST).collect();
    let mut result = 0;
    for (bi, &code) in b.iter().enumerate() {
        let mut distance = bi * MOVE_COST;
        result = distance;
        let mut minimum = usize::MAX;
        for (i, &ac) in a.iter().enumerate() {
            let substitute = distance + substitution_cost(code, ac);
            distance = buffer[i];
            let insert_delete = result.min(distance) + MOVE_COST;
            result = insert_delete.min(substitute);
            buffer[i] = result;
            minimum = minimum.min(result);
        }
        if minimum > max_cost {
            return max_cost + 1;
        }
    }
    result
}

/// CPython's `_Py_CalculateSuggestions` over a sorted candidate list.
pub fn suggest(name: &str, candidates: &[String]) -> Option<String> {
    let mut sorted: Vec<&String> = candidates.iter().collect();
    sorted.sort();
    sorted.dedup();
    if sorted.len() > 750 {
        return None;
    }
    let mut best: Option<&String> = None;
    let mut best_d = usize::MAX;
    for item in sorted {
        if item == name {
            continue;
        }
        let max = (name.len() + item.len() + 3) * MOVE_COST / 6;
        let max = max.min(best_d.saturating_sub(1));
        let d = levenshtein(name.as_bytes(), item.as_bytes(), max);
        if d > max {
            continue;
        }
        if best.is_none() || d < best_d {
            best = Some(item);
            best_d = d;
        }
    }
    best.cloned()
}

// ---------- format spec mini-language ----------

#[derive(Default, Debug)]
pub struct Spec {
    pub fill: Option<char>,
    pub align: Option<char>,
    pub sign: Option<char>,
    pub z: bool,
    pub alt: bool,
    pub zero: bool,
    pub width: Option<usize>,
    pub grouping: Option<char>,
    pub precision: Option<usize>,
    pub ty: Option<char>,
}

pub fn parse_spec(spec: &str) -> PyResult<Spec> {
    let c: Vec<char> = spec.chars().collect();
    let mut s = Spec::default();
    let mut i = 0;
    let is_align = |ch: char| matches!(ch, '<' | '>' | '=' | '^');
    if c.len() >= 2 && is_align(c[1]) {
        s.fill = Some(c[0]);
        s.align = Some(c[1]);
        i = 2;
    } else if !c.is_empty() && is_align(c[0]) {
        s.align = Some(c[0]);
        i = 1;
    }
    if i < c.len() && matches!(c[i], '+' | '-' | ' ') {
        s.sign = Some(c[i]);
        i += 1;
    }
    if i < c.len() && c[i] == 'z' {
        s.z = true;
        i += 1;
    }
    if i < c.len() && c[i] == '#' {
        s.alt = true;
        i += 1;
    }
    if i < c.len() && c[i] == '0' {
        s.zero = true;
        i += 1;
    }
    let start = i;
    while i < c.len() && c[i].is_ascii_digit() {
        i += 1;
    }
    if i > start {
        let w: String = c[start..i].iter().collect();
        s.width = Some(
            w.parse()
                .map_err(|_| value_err("Too many decimal digits in format string"))?,
        );
    }
    if i < c.len() && (c[i] == ',' || c[i] == '_') {
        s.grouping = Some(c[i]);
        i += 1;
        if i < c.len() && (c[i] == ',' || c[i] == '_') {
            return Err(value_err("Cannot specify both ',' and '_'."));
        }
    }
    if i < c.len() && c[i] == '.' {
        i += 1;
        let start = i;
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return Err(value_err("Format specifier missing precision"));
        }
        let p: String = c[start..i].iter().collect();
        s.precision = Some(
            p.parse()
                .map_err(|_| value_err("Too many decimal digits in format string"))?,
        );
    }
    if i < c.len() {
        s.ty = Some(c[i]);
        i += 1;
    }
    if i < c.len() {
        return Err(value_err("Invalid format specifier"));
    }
    Ok(s)
}

fn pad(body: &str, sign: &str, prefix: &str, s: &Spec, default_align: char) -> String {
    let fill = s.fill.unwrap_or(if s.zero && s.align.is_none() {
        '0'
    } else {
        ' '
    });
    let align = s
        .align
        .unwrap_or(if s.zero && s.fill.is_none() && default_align == '>' {
            '='
        } else {
            default_align
        });
    let content_len = sign.chars().count() + prefix.chars().count() + body.chars().count();
    let width = s.width.unwrap_or(0);
    if content_len >= width {
        return format!("{sign}{prefix}{body}");
    }
    let n = width - content_len;
    let f = |k: usize| fill.to_string().repeat(k);
    match align {
        '<' => format!("{sign}{prefix}{body}{}", f(n)),
        '^' => format!("{}{sign}{prefix}{body}{}", f(n / 2), f(n - n / 2)),
        '=' => format!("{sign}{prefix}{}{body}", f(n)),
        _ => format!("{}{sign}{prefix}{body}", f(n)),
    }
}

fn group_digits(int_part: &str, sep: char, every: usize) -> String {
    let chars: Vec<char> = int_part.chars().collect();
    let mut out = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(every) {
            out.push(sep);
        }
        out.push(*ch);
    }
    out
}

/// Zero padding with grouping inserts separators into the padding too.
fn pad_grouped(
    digits: &str,
    sign: &str,
    prefix: &str,
    s: &Spec,
    sep: char,
    every: usize,
) -> String {
    let fill_zero = s.zero && s.fill.is_none() && s.align.is_none()
        || (s.fill == Some('0') && s.align == Some('='));
    if !fill_zero {
        return pad(digits, sign, prefix, s, '>');
    }
    let width = s.width.unwrap_or(0);
    let (int_part, rest) = match digits.find(|c: char| !c.is_ascii_digit() && c != sep) {
        Some(p) => (&digits[..p], &digits[p..]),
        None => (digits, ""),
    };
    let raw: String = int_part.chars().filter(|c| *c != sep).collect();
    let mut raw = raw;
    loop {
        let g = group_digits(&raw, sep, every);
        let total = sign.len() + prefix.len() + g.chars().count() + rest.chars().count();
        if total >= width {
            return format!("{sign}{prefix}{g}{rest}");
        }
        raw.insert(0, '0');
    }
}

pub fn format_value(vm: &mut Vm, v: &Value, spec: &str) -> PyResult<String> {
    match v {
        Value::Str(st) => {
            let s = parse_spec(spec)?;
            if let Some(t) = s.ty {
                if t != 's' {
                    return Err(value_err(format!(
                        "Unknown format code '{t}' for object of type 'str'"
                    )));
                }
            }
            if s.sign.is_some() {
                return Err(value_err("Sign not allowed in string format specifier"));
            }
            if s.alt {
                return Err(value_err(
                    "Alternate form (#) not allowed in string format specifier",
                ));
            }
            if let Some(g) = s.grouping {
                return Err(value_err(format!("Cannot specify '{g}' with 's'.")));
            }
            if s.align == Some('=') {
                return Err(value_err(
                    "'=' alignment not allowed in string format specifier",
                ));
            }
            let body: String = match s.precision {
                Some(p) => st.s.chars().take(p).collect(),
                None => st.s.clone(),
            };
            let mut s2 = s;
            if s2.zero && s2.fill.is_none() && s2.align.is_none() {
                s2.fill = Some('0');
                s2.align = Some('<');
            }
            Ok(pad(&body, "", "", &s2, '<'))
        }
        Value::Int(_) | Value::Big(_) | Value::Bool(_) => {
            if spec.is_empty() {
                return vm.str_of(v);
            }
            let s = parse_spec(spec)?;
            let big = match v {
                Value::Int(i) => BigInt::from_i64(*i),
                Value::Bool(b) => BigInt::from_i64(*b as i64),
                Value::Big(b) => (**b).clone(),
                _ => unreachable!(),
            };
            if let Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') = s.ty {
                let f = big
                    .to_f64()
                    .ok_or_else(|| err("OverflowError", "int too large to convert to float"))?;
                return format_float(f, &s);
            }
            format_int(&big, &s)
        }
        Value::Float(f) => {
            if spec.is_empty() {
                return Ok(float_repr(*f));
            }
            let s = parse_spec(spec)?;
            format_float(*f, &s)
        }
        Value::Complex(r, i) => {
            if spec.is_empty() {
                return Ok(complex_repr(*r, *i));
            }
            let s = parse_spec(spec)?;
            let re = format_float(
                *r,
                &Spec {
                    width: None,
                    ..parse_spec(spec)?
                },
            )?;
            let im = format_float(
                *i,
                &Spec {
                    width: None,
                    sign: Some('+'),
                    ..parse_spec(spec)?
                },
            )?;
            Ok(pad(&format!("{re}{im}j"), "", "", &s, '>'))
        }
        _ => {
            if spec.is_empty() {
                return vm.str_of(v);
            }
            Err(type_err(format!(
                "unsupported format string passed to {}.__format__",
                vm.type_name(v)
            )))
        }
    }
}

fn format_int(v: &BigInt, s: &Spec) -> PyResult<String> {
    if s.precision.is_some() {
        return Err(value_err(
            "Precision not allowed in integer format specifier",
        ));
    }
    let neg = v.is_negative();
    let mag = v.abs();
    let (digits, prefix) = match s.ty {
        None | Some('d') | Some('n') => (mag.to_str_radix(10), ""),
        Some('b') => (mag.to_str_radix(2), "0b"),
        Some('o') => (mag.to_str_radix(8), "0o"),
        Some('x') => (mag.to_str_radix(16), "0x"),
        Some('X') => (mag.to_str_radix(16).to_uppercase(), "0X"),
        Some('c') => {
            let code = v.to_i64().unwrap_or(-1);
            let ch = u32::try_from(code)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| err("OverflowError", "%c arg not in range(0x110000)"))?;
            return Ok(pad(&ch.to_string(), "", "", s, '<'));
        }
        Some(t) => {
            return Err(value_err(format!(
                "Unknown format code '{t}' for object of type 'int'"
            )))
        }
    };
    let sign = if neg {
        "-"
    } else {
        match s.sign {
            Some('+') => "+",
            Some(' ') => " ",
            _ => "",
        }
    };
    let prefix = if s.alt { prefix } else { "" };
    if let Some(g) = s.grouping {
        let every = if matches!(s.ty, Some('b' | 'o' | 'x' | 'X')) {
            4
        } else {
            3
        };
        if g == ',' && every == 4 {
            return Err(value_err(format!(
                "Cannot specify ',' with '{}'.",
                s.ty.unwrap()
            )));
        }
        let grouped = group_digits(&digits, g, every);
        return Ok(pad_grouped(&grouped, sign, prefix, s, g, every));
    }
    Ok(pad(&digits, sign, prefix, s, '>'))
}

/// `{:.Ne}` in Python style (`e+05`).
fn fmt_exp(f: f64, prec: usize, upper: bool) -> String {
    let s = format!("{:.*e}", prec, f);
    let (m, e) = s.split_once('e').unwrap();
    let e: i32 = e.parse().unwrap();
    let r = format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    if upper {
        r.to_uppercase()
    } else {
        r
    }
}

fn strip_zeros(s: &str) -> String {
    if let Some(epos) = s.find(['e', 'E']) {
        let (m, e) = s.split_at(epos);
        return format!("{}{e}", strip_zeros(m));
    }
    if s.contains('.') {
        let t = s.trim_end_matches('0');
        t.trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// Decimal exponent of `f` after rounding to `p` significant digits.
fn exp_after_round(f: f64, p: usize) -> i32 {
    let s = format!("{:.*e}", p.saturating_sub(1), f.abs());
    s.split_once('e').unwrap().1.parse().unwrap()
}

/// The digits of `f` (non-negative) per the presentation type, without sign.
fn float_body(f: f64, ty: Option<char>, prec: Option<usize>, alt: bool) -> String {
    if f.is_nan() {
        return if matches!(ty, Some('E' | 'F' | 'G')) {
            "NAN"
        } else {
            "nan"
        }
        .into();
    }
    if f.is_infinite() {
        return if matches!(ty, Some('E' | 'F' | 'G')) {
            "INF"
        } else {
            "inf"
        }
        .into();
    }
    match ty {
        Some('f') | Some('F') => {
            let p = prec.unwrap_or(6);
            let s = format!("{:.*}", p, f);
            if alt && p == 0 {
                format!("{s}.")
            } else {
                s
            }
        }
        Some('e') | Some('E') => {
            let p = prec.unwrap_or(6);
            let mut s = fmt_exp(f, p, ty == Some('E'));
            if alt && p == 0 {
                let pos = s.find(['e', 'E']).unwrap();
                s.insert(pos, '.');
            }
            s
        }
        Some('%') => {
            let p = prec.unwrap_or(6);
            format!("{:.*}%", p, f * 100.0)
        }
        Some('g') | Some('G') | Some('n') => {
            let mut p = prec.unwrap_or(6);
            if p == 0 {
                p = 1;
            }
            if f == 0.0 {
                let s = if alt {
                    format!("{:.*}", p - 1, 0.0)
                } else {
                    "0".into()
                };
                return s;
            }
            let exp = exp_after_round(f, p);
            let s = if exp >= -4 && (exp as i64) < p as i64 {
                format!("{:.*}", (p as i64 - 1 - exp as i64).max(0) as usize, f)
            } else {
                fmt_exp(f, p - 1, ty == Some('G'))
            };
            if alt {
                s
            } else {
                strip_zeros(&s)
            }
        }
        None => match prec {
            None => float_repr(f),
            Some(p) => {
                let p = p.max(1);
                let exp = if f == 0.0 { 0 } else { exp_after_round(f, p) };
                if exp >= -4 && (exp as i64) < p as i64 - 1 {
                    let after = (p as i64 - 1 - exp as i64).max(0) as usize;
                    let s = strip_zeros(&format!("{:.*}", after, f));
                    if s.contains('.') {
                        s
                    } else {
                        format!("{s}.0")
                    }
                } else {
                    strip_zeros(&fmt_exp(f, p - 1, false))
                }
            }
        },
        _ => unreachable!(),
    }
}

pub fn format_float(f: f64, s: &Spec) -> PyResult<String> {
    if let Some(t) = s.ty {
        if !matches!(t, 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'n' | '%') {
            return Err(value_err(format!(
                "Unknown format code '{t}' for object of type 'float'"
            )));
        }
    }
    let neg = f.is_sign_negative() && !f.is_nan();
    let mut body = float_body(f.abs(), s.ty, s.precision, s.alt);
    let negative_zero = s.z && neg && body.chars().all(|c| matches!(c, '0' | '.' | '%'));
    let sign = if neg && !negative_zero {
        "-"
    } else {
        match s.sign {
            Some('+') => "+",
            Some(' ') => " ",
            _ => "",
        }
    };
    if let Some(g) = s.grouping {
        let (int_part, rest) = match body.find(|c: char| !c.is_ascii_digit()) {
            Some(p) => (body[..p].to_string(), body[p..].to_string()),
            None => (body.clone(), String::new()),
        };
        body = format!("{}{}", group_digits(&int_part, g, 3), rest);
        return Ok(pad_grouped(&body, sign, "", s, g, 3));
    }
    Ok(pad(&body, sign, "", s, '>'))
}

// ---------- printf-style % formatting ----------

pub fn percent_format(vm: &mut Vm, fmt: &Value, args: &Value) -> PyResult<String> {
    let Value::Str(fs) = fmt else { unreachable!() };
    let chars: Vec<char> = fs.s.chars().collect();
    let base_tuple = match args {
        Value::Instance(i) => match &*i.native.borrow() {
            NativeData::Base(Value::Tuple(t)) => Some((**t).clone()),
            _ => None,
        },
        _ => None,
    };
    let (items, mapping): (Vec<Value>, Option<Value>) = match args {
        Value::Tuple(t) => ((**t).clone(), None),
        _ if base_tuple.is_some() => (base_tuple.unwrap_or_default(), None),
        Value::Dict(_) => (vec![args.clone()], Some(args.clone())),
        Value::Instance(i)
            if i.class().lookup("__getitem__").is_some()
                && !matches!(&*i.native.borrow(), NativeData::Base(Value::Tuple(_))) =>
        {
            (vec![args.clone()], Some(args.clone()))
        }
        other => (vec![other.clone()], None),
    };
    let mut out = String::new();
    let mut argi = 0;
    let mut i = 0;
    let mut used_mapping = false;
    let next_arg = |argi: &mut usize| -> PyResult<Value> {
        let v = items
            .get(*argi)
            .cloned()
            .ok_or_else(|| type_err("not enough arguments for format string"))?;
        *argi += 1;
        Ok(v)
    };
    while i < chars.len() {
        let c = chars[i];
        if c != '%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            return Err(value_err("incomplete format"));
        }
        let mut key: Option<String> = None;
        if chars[i] == '(' {
            let mut depth = 1;
            let start = i + 1;
            i += 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '(' {
                    depth += 1;
                } else if chars[i] == ')' {
                    depth -= 1;
                }
                i += 1;
            }
            if depth > 0 {
                return Err(value_err("incomplete format key"));
            }
            key = Some(chars[start..i - 1].iter().collect());
        }
        let mut spec = Spec::default();
        let mut left = false;
        while i < chars.len() && matches!(chars[i], '-' | '+' | ' ' | '#' | '0') {
            match chars[i] {
                '-' => left = true,
                '+' => spec.sign = Some('+'),
                ' ' => {
                    if spec.sign.is_none() {
                        spec.sign = Some(' ')
                    }
                }
                '#' => spec.alt = true,
                _ => spec.zero = true,
            }
            i += 1;
        }
        // Width
        if i < chars.len() && chars[i] == '*' {
            let w = next_arg(&mut argi)?;
            let w = vm.index_of(&w)?;
            if w < 0 {
                left = true;
            }
            spec.width = Some(w.unsigned_abs() as usize);
            i += 1;
        } else {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i > start {
                spec.width = Some(
                    chars[start..i]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0),
                );
            }
        }
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            if i < chars.len() && chars[i] == '*' {
                let p = next_arg(&mut argi)?;
                spec.precision = Some(vm.index_of(&p)?.max(0) as usize);
                i += 1;
            } else {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                spec.precision = Some(
                    chars[start..i]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0),
                );
            }
        }
        while i < chars.len() && matches!(chars[i], 'h' | 'l' | 'L') {
            i += 1;
        }
        if i >= chars.len() {
            return Err(value_err("incomplete format"));
        }
        let conv = chars[i];
        i += 1;
        if conv == '%' {
            out.push('%');
            continue;
        }
        let arg = match &key {
            Some(k) => {
                let m = mapping
                    .clone()
                    .ok_or_else(|| type_err("format requires a mapping"))?;
                used_mapping = true;
                vm.getitem(&m, &Value::str(k))?
            }
            None => next_arg(&mut argi)?,
        };
        if left {
            spec.align = Some('<');
            spec.zero = false;
        }
        let piece = match conv {
            's' | 'r' | 'a' => {
                let s = match conv {
                    's' => vm.str_of(&arg)?,
                    'r' => vm.repr(&arg)?,
                    _ => ascii_escape(&vm.repr(&arg)?),
                };
                let s: String = match spec.precision {
                    Some(p) => s.chars().take(p).collect(),
                    None => s,
                };
                let sp = Spec {
                    align: Some(if left { '<' } else { '>' }),
                    width: spec.width,
                    ..Spec::default()
                };
                pad(&s, "", "", &sp, '>')
            }
            'd' | 'i' | 'u' => {
                let n = match &arg {
                    Value::Float(f) => {
                        if !f.is_finite() {
                            return Err(err(
                                "OverflowError",
                                "cannot convert float infinity to integer",
                            ));
                        }
                        Value::big(BigInt::from_f64(*f))
                    }
                    Value::Int(_) | Value::Big(_) | Value::Bool(_) => arg.clone(),
                    _ => match vm.as_num(&arg) {
                        Some(crate::ops::Num::I(i)) => Value::Int(i),
                        Some(crate::ops::Num::B(b)) => Value::Big(b),
                        Some(crate::ops::Num::F(f)) => Value::big(BigInt::from_f64(f)),
                        _ => {
                            if let Some(r) = vm.call_special(&arg, "__int__", vec![])? {
                                r
                            } else {
                                return Err(type_err(format!(
                                    "%{conv} format: a real number is required, not {}",
                                    vm.type_name(&arg)
                                )));
                            }
                        }
                    },
                };
                let big = match n {
                    Value::Int(i) => BigInt::from_i64(i),
                    Value::Bool(b) => BigInt::from_i64(b as i64),
                    Value::Big(b) => (*b).clone(),
                    _ => BigInt::zero(),
                };
                let mut sp = spec_int(&spec, left);
                sp.ty = Some('d');
                let digits = match spec.precision {
                    Some(p) => {
                        let d = big.abs().to_str_radix(10);
                        format!("{}{}", "0".repeat(p.saturating_sub(d.len())), d)
                    }
                    None => big.abs().to_str_radix(10),
                };
                let sign = if big.is_negative() {
                    "-"
                } else {
                    match spec.sign {
                        Some('+') => "+",
                        Some(' ') => " ",
                        _ => "",
                    }
                };
                sp.sign = None;
                pad(&digits, sign, "", &sp, '>')
            }
            'x' | 'X' | 'o' => {
                let big = match &arg {
                    Value::Int(i) => BigInt::from_i64(*i),
                    Value::Bool(b) => BigInt::from_i64(*b as i64),
                    Value::Big(b) => (**b).clone(),
                    _ => {
                        return Err(type_err(format!(
                            "%{conv} format: an integer is required, not {}",
                            vm.type_name(&arg)
                        )))
                    }
                };
                let radix = if conv == 'o' { 8 } else { 16 };
                let mut digits = big.abs().to_str_radix(radix);
                if conv == 'X' {
                    digits = digits.to_uppercase();
                }
                if let Some(p) = spec.precision {
                    digits = format!("{}{}", "0".repeat(p.saturating_sub(digits.len())), digits);
                }
                let prefix = if spec.alt {
                    match conv {
                        'o' => "0o",
                        'x' => "0x",
                        _ => "0X",
                    }
                } else {
                    ""
                };
                let sign = if big.is_negative() {
                    "-"
                } else {
                    match spec.sign {
                        Some('+') => "+",
                        Some(' ') => " ",
                        _ => "",
                    }
                };
                let sp = spec_int(&spec, left);
                pad(&digits, sign, prefix, &sp, '>')
            }
            'e' | 'E' | 'f' | 'F' | 'g' | 'G' => {
                let f = match vm.as_num(&arg) {
                    Some(n) => vm.num_to_f64(&n)?,
                    None => {
                        if let Some(r) = vm.call_special(&arg, "__float__", vec![])? {
                            match r {
                                Value::Float(f) => f,
                                _ => 0.0,
                            }
                        } else {
                            return Err(type_err(format!(
                                "must be real number, not {}",
                                vm.type_name(&arg)
                            )));
                        }
                    }
                };
                let mut sp = spec_int(&spec, left);
                sp.ty = Some(conv);
                sp.precision = Some(spec.precision.unwrap_or(6));
                sp.alt = spec.alt;
                sp.sign = spec.sign;
                format_float(f, &sp)?
            }
            'c' => {
                let s = match &arg {
                    Value::Int(i) => char::from_u32(*i as u32)
                        .ok_or_else(|| err("OverflowError", "%c arg not in range(0x110000)"))?
                        .to_string(),
                    Value::Str(s) if s.nchars == 1 => s.s.clone(),
                    _ => return Err(type_err("%c requires int or char")),
                };
                let sp = Spec {
                    align: Some(if left { '<' } else { '>' }),
                    width: spec.width,
                    ..Spec::default()
                };
                pad(&s, "", "", &sp, '>')
            }
            other => {
                return Err(value_err(format!(
                    "unsupported format character '{}' (0x{:x}) at index {}",
                    other,
                    other as u32,
                    i - 1
                )))
            }
        };
        out.push_str(&piece);
    }
    if argi < items.len() && !used_mapping && mapping.is_none() {
        return Err(type_err(
            "not all arguments converted during string formatting",
        ));
    }
    Ok(out)
}

fn spec_int(spec: &Spec, left: bool) -> Spec {
    Spec {
        fill: if spec.zero && !left { Some('0') } else { None },
        align: Some(if left {
            '<'
        } else if spec.zero {
            '='
        } else {
            '>'
        }),
        width: spec.width,
        ..Spec::default()
    }
}

// ---------- str.format ----------

pub fn str_format(
    vm: &mut Vm,
    template: &str,
    args: &[Value],
    kwargs: &[(Rc<str>, Value)],
) -> PyResult<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut auto = 0usize;
    let mut mode = 0u8; // 0 unknown, 1 auto, 2 manual
    str_format_inner(vm, &chars, args, kwargs, &mut out, &mut auto, &mut mode, 0)?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn str_format_inner(
    vm: &mut Vm,
    chars: &[char],
    args: &[Value],
    kwargs: &[(Rc<str>, Value)],
    out: &mut String,
    auto: &mut usize,
    mode: &mut u8,
    depth: u32,
) -> PyResult<()> {
    if depth > 2 {
        return Err(value_err("Max string recursion exceeded"));
    }
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '}' {
            if chars.get(i + 1) == Some(&'}') {
                out.push('}');
                i += 2;
                continue;
            }
            return Err(value_err("Single '}' encountered in format string"));
        }
        if c != '{' {
            out.push(c);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'{') {
            out.push('{');
            i += 2;
            continue;
        }
        // Find the matching close brace.
        let mut j = i + 1;
        let mut level = 1;
        while j < chars.len() {
            match chars[j] {
                '{' => level += 1,
                '}' => {
                    level -= 1;
                    if level == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if j >= chars.len() {
            return Err(value_err("expected '}' before end of string"));
        }
        let field: Vec<char> = chars[i + 1..j].to_vec();
        i = j + 1;
        // Split field_name[!conv][:spec]
        let mut name_end = field.len();
        let mut bracket = false;
        for (k, ch) in field.iter().enumerate() {
            match ch {
                '[' => bracket = true,
                ']' => bracket = false,
                '!' | ':' if !bracket => {
                    name_end = k;
                    break;
                }
                _ => {}
            }
        }
        let name: String = field[..name_end].iter().collect();
        let mut conv = None;
        let mut spec_chars: &[char] = &[];
        let mut k = name_end;
        if k < field.len() && field[k] == '!' {
            conv = field.get(k + 1).copied();
            if conv.is_none() {
                return Err(value_err(
                    "end of string while looking for conversion specifier",
                ));
            }
            k += 2;
            if k < field.len() && field[k] != ':' {
                return Err(value_err("expected ':' after conversion specifier"));
            }
        }
        if k < field.len() && field[k] == ':' {
            spec_chars = &field[k + 1..];
        }
        // Resolve the value.
        let (first, rest) = split_field_name(&name);
        let mut value = if first.is_empty() {
            if *mode == 2 {
                return Err(value_err(
                    "cannot switch from manual field specification to automatic field numbering",
                ));
            }
            *mode = 1;
            let idx = *auto;
            *auto += 1;
            args.get(idx).cloned().ok_or_else(|| {
                err(
                    "IndexError",
                    format!("Replacement index {idx} out of range for positional args tuple"),
                )
            })?
        } else if let Ok(idx) = first.parse::<usize>() {
            if *mode == 1 {
                return Err(value_err(
                    "cannot switch from automatic field numbering to manual field specification",
                ));
            }
            *mode = 2;
            args.get(idx).cloned().ok_or_else(|| {
                err(
                    "IndexError",
                    format!("Replacement index {idx} out of range for positional args tuple"),
                )
            })?
        } else {
            kwargs
                .iter()
                .find(|(k, _)| **k == *first)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| err_args("KeyError", vec![Value::str(&first)]))?
        };
        for part in rest {
            match part {
                FieldPart::Attr(a) => value = vm.getattr_str(&value, &a)?,
                FieldPart::Index(ix) => {
                    let key = match ix.parse::<i64>() {
                        Ok(n) => Value::Int(n),
                        Err(_) => Value::str(&ix),
                    };
                    value = vm.getitem(&value, &key)?;
                }
            }
        }
        let value = match conv {
            None => value,
            Some('s') => Value::string(vm.str_of(&value)?),
            Some('r') => Value::string(vm.repr(&value)?),
            Some('a') => Value::string(ascii_escape(&vm.repr(&value)?)),
            Some(c) => return Err(value_err(format!("Unknown conversion specifier {c}"))),
        };
        let mut spec = String::new();
        str_format_inner(
            vm,
            spec_chars,
            args,
            kwargs,
            &mut spec,
            auto,
            mode,
            depth + 1,
        )?;
        out.push_str(&vm.format(&value, &spec)?);
    }
    Ok(())
}

enum FieldPart {
    Attr(String),
    Index(String),
}

fn split_field_name(name: &str) -> (String, Vec<FieldPart>) {
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
        i += 1;
    }
    let first: String = chars[..i].iter().collect();
    let mut parts = vec![];
    while i < chars.len() {
        if chars[i] == '.' {
            let start = i + 1;
            i += 1;
            while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                i += 1;
            }
            parts.push(FieldPart::Attr(chars[start..i].iter().collect()));
        } else if chars[i] == '[' {
            let start = i + 1;
            while i < chars.len() && chars[i] != ']' {
                i += 1;
            }
            parts.push(FieldPart::Index(chars[start..i].iter().collect()));
            i += 1;
        } else {
            i += 1;
        }
    }
    (first, parts)
}
