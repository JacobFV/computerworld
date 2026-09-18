//! Number <-> string conversions with ECMAScript semantics.

/// Shortest round-trip digits of a finite positive `x`: (digits, n) with
/// x = 0.d1d2... * 10^n.
fn shortest(x: f64) -> (Vec<u8>, i32) {
    let s = format!("{:e}", x);
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: Vec<u8> = mant
        .bytes()
        .filter(|b| b.is_ascii_digit())
        .map(|b| b - b'0')
        .collect();
    let mut d = digits;
    while d.len() > 1 && *d.last().unwrap() == 0 {
        d.pop();
    }
    (d, exp + 1)
}

/// Exact decimal digits of a finite positive `x`: (digits, e) with the first
/// digit at 10^e.
fn exact(x: f64) -> (Vec<u8>, i32) {
    let s = format!("{:.1100e}", x);
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let mut d: Vec<u8> = mant
        .bytes()
        .filter(|b| b.is_ascii_digit())
        .map(|b| b - b'0')
        .collect();
    while d.len() > 1 && *d.last().unwrap() == 0 {
        d.pop();
    }
    (d, exp)
}

/// Rounds `digits` (first digit at 10^e) half-up to `keep` digits.
/// Returns the new digits (exactly `keep` long, or 1 when keep == 0 and it
/// rounded up) and exponent.
fn round_half_up(digits: &[u8], e: i32, keep: i32) -> (Vec<u8>, i32) {
    if keep < 0 {
        return (vec![], e);
    }
    let keep = keep as usize;
    let mut out: Vec<u8> = digits.iter().take(keep).copied().collect();
    while out.len() < keep {
        out.push(0);
    }
    let round_up = digits.get(keep).map(|&d| d >= 5).unwrap_or(false);
    if round_up {
        let mut i = out.len();
        loop {
            if i == 0 {
                out.insert(0, 1);
                if keep > 0 {
                    out.pop();
                }
                return (out, e + 1);
            }
            i -= 1;
            if out[i] == 9 {
                out[i] = 0;
            } else {
                out[i] += 1;
                break;
            }
        }
    }
    (out, e)
}

fn digits_str(d: &[u8]) -> String {
    d.iter().map(|&x| (b'0' + x) as char).collect()
}

pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }
    let (d, n) = shortest(x);
    let k = d.len() as i32;
    let ds = digits_str(&d);
    if k <= n && n <= 21 {
        let mut s = ds;
        for _ in 0..(n - k) {
            s.push('0');
        }
        s
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &ds[..n as usize], &ds[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{}", "0".repeat((-n) as usize), ds)
    } else {
        let e = n - 1;
        let sign = if e >= 0 { '+' } else { '-' };
        if k == 1 {
            format!("{}e{}{}", ds, sign, e.abs())
        } else {
            format!("{}.{}e{}{}", &ds[..1], &ds[1..], sign, e.abs())
        }
    }
}

fn next_double(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let b = x.to_bits();
    if x > 0.0 {
        f64::from_bits(b + 1)
    } else {
        f64::from_bits(b - 1)
    }
}

/// `Number.prototype.toString(radix)` for radix != 10 (V8's algorithm).
pub fn to_radix(value: f64, radix: u32) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if value == 0.0 {
        return "0".into();
    }
    let chars = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let neg = value < 0.0;
    let value = value.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let mut delta = 0.5 * (next_double(value) - value);
    delta = delta.max(next_double(0.0));
    let mut frac_buf: Vec<u8> = vec![];
    if fraction >= delta {
        loop {
            fraction *= radix as f64;
            delta *= radix as f64;
            let digit = fraction as u32;
            frac_buf.push(chars[digit as usize]);
            fraction -= digit as f64;
            if (fraction > 0.5 || (fraction == 0.5 && (digit & 1) == 1)) && fraction + delta > 1.0 {
                // Round up with carry.
                loop {
                    match frac_buf.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(c) => {
                            let d = if c > b'9' {
                                (c - b'a' + 10) as u32
                            } else {
                                (c - b'0') as u32
                            };
                            if d + 1 < radix {
                                frac_buf.push(chars[(d + 1) as usize]);
                                break;
                            }
                        }
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    let mut int_buf: Vec<u8> = vec![];
    let exponent = |v: f64| -> i32 {
        // Binary exponent of v as significand(53 bits) * 2^e.
        if v == 0.0 {
            return -1074;
        }
        let bits = v.to_bits();
        let biased = ((bits >> 52) & 0x7ff) as i32;
        if biased == 0 {
            -1074
        } else {
            biased - 1075
        }
    };
    while exponent(integer / radix as f64) > 0 {
        integer /= radix as f64;
        int_buf.push(b'0');
    }
    loop {
        let rem = integer % radix as f64;
        int_buf.push(chars[rem as usize]);
        integer = (integer - rem) / radix as f64;
        if integer <= 0.0 {
            break;
        }
    }
    int_buf.reverse();
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(std::str::from_utf8(&int_buf).unwrap());
    if !frac_buf.is_empty() {
        s.push('.');
        s.push_str(std::str::from_utf8(&frac_buf).unwrap());
    }
    s
}

pub fn to_fixed(x: f64, f: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 || x.is_infinite() {
        return number_to_string(x);
    }
    let neg = x < 0.0;
    let ax = x.abs();
    let mut body = if ax == 0.0 {
        if f == 0 {
            "0".to_string()
        } else {
            format!("0.{}", "0".repeat(f))
        }
    } else {
        let (d, e) = exact(ax);
        let keep = e + 1 + f as i32;
        let (r, e2) = round_half_up(&d, e, keep);
        // Value = 0.r * 10^(e2+1) ; build an integer string of all digits
        // down to 10^-f.
        let mut int_digits: Vec<u8> = vec![];
        if r.is_empty() {
            int_digits = vec![0; f + 1];
        } else {
            // Digits positions: r[i] at 10^(e2 - i). Need positions from max(e2,0) down to -f.
            let top = e2.max(0);
            let mut p = top;
            while p >= -(f as i32) {
                let idx = e2 - p;
                let dgt = if idx >= 0 && (idx as usize) < r.len() {
                    r[idx as usize]
                } else {
                    0
                };
                int_digits.push(dgt);
                p -= 1;
            }
        }
        let s = digits_str(&int_digits);
        if f == 0 {
            s
        } else {
            let split = s.len() - f;
            let (a, b) = s.split_at(split);
            let a = if a.is_empty() { "0" } else { a };
            format!("{a}.{b}")
        }
    };
    if neg && body.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
        body.insert(0, '-');
    } else if neg {
        // (-0.0001).toFixed(2) is "-0.00" in JS.
        if x != 0.0 {
            body.insert(0, '-');
        }
    }
    body
}

pub fn to_exponential(x: f64, f: Option<usize>) -> String {
    if !x.is_finite() {
        return number_to_string(x);
    }
    let neg = x < 0.0;
    let ax = x.abs();
    let (d, e) = if ax == 0.0 {
        (vec![0u8; f.unwrap_or(0) + 1], 0)
    } else {
        match f {
            None => {
                let (d, n) = shortest(ax);
                (d, n - 1)
            }
            Some(f) => {
                let (d, e) = exact(ax);
                round_half_up(&d, e, f as i32 + 1)
            }
        }
    };
    let ds = digits_str(&d);
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(&ds[..1]);
    if ds.len() > 1 {
        s.push('.');
        s.push_str(&ds[1..]);
    }
    s.push('e');
    s.push(if e >= 0 { '+' } else { '-' });
    s.push_str(&e.abs().to_string());
    s
}

pub fn to_precision(x: f64, p: usize) -> String {
    if !x.is_finite() {
        return number_to_string(x);
    }
    let neg = x < 0.0;
    let ax = x.abs();
    let (d, e) = if ax == 0.0 {
        (vec![0u8; p], 0)
    } else {
        let (d, e) = exact(ax);
        round_half_up(&d, e, p as i32)
    };
    let ds = digits_str(&d);
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    if e < -6 || e >= p as i32 {
        s.push_str(&ds[..1]);
        if p > 1 {
            s.push('.');
            s.push_str(&ds[1..]);
        }
        s.push('e');
        s.push(if e >= 0 { '+' } else { '-' });
        s.push_str(&e.abs().to_string());
    } else if e >= 0 {
        let e = e as usize;
        s.push_str(&ds[..e + 1]);
        if p > e + 1 {
            s.push('.');
            s.push_str(&ds[e + 1..]);
        }
    } else {
        s.push_str("0.");
        s.push_str(&"0".repeat((-e - 1) as usize));
        s.push_str(&ds);
    }
    s
}

pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\x0b' | '\x0c' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// ToNumber applied to a string.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_js_whitespace);
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.is_empty() || !rest.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            let mut v = 0f64;
            for c in rest.chars() {
                v = v * radix as f64 + c.to_digit(radix).unwrap() as f64;
            }
            return v;
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if valid_decimal(t) {
        t.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn valid_decimal(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut ed = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            ed += 1;
        }
        if ed == 0 {
            return false;
        }
    }
    i == b.len()
}

/// Longest valid decimal prefix, for `parseFloat`.
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(is_js_whitespace);
    for (lit, v) in [
        ("Infinity", f64::INFINITY),
        ("+Infinity", f64::INFINITY),
        ("-Infinity", f64::NEG_INFINITY),
    ] {
        if t.starts_with(lit) {
            return v;
        }
    }
    let b = t.as_bytes();
    let mut best = None;
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if digits > 0 {
        best = Some(i);
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
        if digits > 0 {
            best = Some(i);
        }
    }
    if digits > 0 && i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let st = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > st {
            best = Some(j);
        }
    }
    match best {
        Some(end) => {
            let txt = &t[..end];
            let txt = txt.strip_suffix('.').unwrap_or(txt);
            let txt = txt.replace("+.", "+0.").replace("-.", "-0.");
            let txt = if txt.starts_with('.') {
                format!("0{txt}")
            } else {
                txt
            };
            txt.parse().unwrap_or(f64::NAN)
        }
        None => f64::NAN,
    }
}

pub fn parse_int(s: &str, radix: i32) -> f64 {
    let mut t = s.trim_start_matches(is_js_whitespace);
    let mut neg = false;
    if let Some(r) = t.strip_prefix('-') {
        neg = true;
        t = r;
    } else if let Some(r) = t.strip_prefix('+') {
        t = r;
    }
    let mut radix = radix;
    let mut strip = true;
    if radix != 0 {
        if !(2..=36).contains(&radix) {
            return f64::NAN;
        }
        if radix != 16 {
            strip = false;
        }
    } else {
        radix = 10;
    }
    if strip && (t.starts_with("0x") || t.starts_with("0X")) {
        t = &t[2..];
        radix = 16;
    }
    let mut v = 0f64;
    let mut any = false;
    let digits: Vec<u32> = t.chars().map_while(|c| c.to_digit(radix as u32)).collect();
    if radix == 10 && !digits.is_empty() {
        let txt: String = digits
            .iter()
            .map(|d| char::from_digit(*d, 10).unwrap())
            .collect();
        let v: f64 = txt.parse().unwrap_or(f64::NAN);
        return if neg { -v } else { v };
    }
    for d in digits {
        v = v * radix as f64 + d as f64;
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if neg {
        -v
    } else {
        v
    }
}

/// ToInt32.
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

pub fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() || x == 0.0 {
        return 0;
    }
    if x.abs() < 2147483648.0 * 2.0 && x.fract() == 0.0 && x >= 0.0 {
        return x as u32;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4294967296.0);
    m as u32
}

/// Canonical array index of a property key, if it is one.
pub fn array_index(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 {
        return None;
    }
    if b[0] == b'0' {
        return if b.len() == 1 { Some(0) } else { None };
    }
    if !b.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: u64 = s.parse().ok()?;
    if v < 4294967295 {
        Some(v as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_like_v8() {
        assert_eq!(number_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1e-7), "1e-7");
        assert_eq!(
            number_to_string(123456789012345680000.0),
            "123456789012345680000"
        );
        assert_eq!(number_to_string(5e-324), "5e-324");
        assert_eq!(number_to_string(-1e-7), "-1e-7");
        assert_eq!(number_to_string(0.0000012), "0.0000012");
        assert_eq!(number_to_string(1.5e300), "1.5e+300");
        assert_eq!(to_fixed(1234.5678, 2), "1234.57");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(0.006, 2), "0.01");
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(-1.5, 0), "-2");
        assert_eq!(to_precision(123.456, 4), "123.5");
        assert_eq!(to_precision(0.000123, 2), "0.00012");
        assert_eq!(to_precision(123456.0, 2), "1.2e+5");
        assert_eq!(to_exponential(123456.0, Some(2)), "1.23e+5");
        assert_eq!(to_exponential(0.00015, None), "1.5e-4");
        assert_eq!(to_radix(255.0, 16), "ff");
        assert_eq!(to_radix(0.5, 2), "0.1");
        assert_eq!(to_radix(-10.25, 2), "-1010.01");
        assert_eq!(to_radix(0.1, 3), "0.0022002200220022002200220022002201");
        assert_eq!(to_radix(1e21, 16), "3635c9adc5dea00000");
        assert_eq!(to_radix(std::f64::consts::PI, 36), "3.53i5ab8p5f");
        assert_eq!(to_fixed(-0.0001, 2), "-0.00");
        assert_eq!(to_fixed(1e-10, 3), "0.000");
        assert_eq!(string_to_number(" 12 "), 12.0);
        assert!(string_to_number("12px").is_nan());
        assert_eq!(parse_float("2.5abc"), 2.5);
        assert_eq!(parse_int("0x1f", 0), 31.0);
        assert_eq!(parse_int("12px", 10), 12.0);
    }
}
