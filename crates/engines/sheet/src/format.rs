//! Excel number format codes (`#,##0.00`, `0%`, `m/d/yyyy`, `$#,##0.00;[Red]-$#,##0.00`,
//! `@`, …): parsing sections and rendering values through them, as TEXT() and cells do.
use crate::date;
use crate::value::{general, Value};

/// A rendered value: its text and the colour a `[Red]`-style section asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    pub color: Option<[u8; 3]>,
}

/// The code Excel stores for a built-in number format id.
pub fn builtin(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        44 => "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}
/// The built-in id of a code, when it is one.
pub fn builtin_id(code: &str) -> Option<u32> {
    (0..=49).find(|id| builtin(*id).is_some_and(|c| c.eq_ignore_ascii_case(code)))
}

/// Split a code into its `;` sections, respecting quotes and brackets.
fn sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut bracket = false;
    let mut escape = false;
    for c in code.chars() {
        if escape {
            out.last_mut().unwrap().push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' if !quoted => {
                escape = true;
                out.last_mut().unwrap().push(c);
                continue;
            }
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            ';' if !quoted && !bracket => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().unwrap().push(c);
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Lit(String),
    /// A digit placeholder: `0`, `#` or `?`.
    Digit(char),
    Point,
    Comma,
    Percent,
    Exp(bool),
    At,
    Date(String),
    AmPm(bool),
    Elapsed(char),
    General,
}
fn tokenize(section: &str) -> (Vec<Tok>, Option<[u8; 3]>) {
    let chars: Vec<char> = section.chars().collect();
    let mut toks = Vec::new();
    let mut color = None;
    let mut i = 0;
    let lit = |toks: &mut Vec<Tok>, s: String| {
        if let Some(Tok::Lit(prev)) = toks.last_mut() {
            prev.push_str(&s);
        } else {
            toks.push(Tok::Lit(s));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().collect();
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("GENERAL") {
            toks.push(Tok::General);
            i += 7;
            continue;
        }
        if upper.starts_with("AM/PM") {
            toks.push(Tok::AmPm(true));
            i += 5;
            continue;
        }
        if upper.starts_with("A/P") {
            toks.push(Tok::AmPm(false));
            i += 3;
            continue;
        }
        match c {
            '"' => {
                let mut s = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    s.push(chars[i]);
                    i += 1;
                }
                lit(&mut toks, s);
                i += 1;
            }
            '\\' => {
                if let Some(n) = chars.get(i + 1) {
                    lit(&mut toks, n.to_string());
                }
                i += 2;
            }
            '_' => {
                lit(&mut toks, " ".into());
                i += 2;
            }
            '*' => i += 2,
            '[' => {
                let end = chars[i..]
                    .iter()
                    .position(|c| *c == ']')
                    .map_or(chars.len(), |p| i + p);
                let inner: String = chars[i + 1..end.min(chars.len())].iter().collect();
                let lower = inner.to_ascii_lowercase();
                match lower.as_str() {
                    "red" => color = Some([255, 0, 0]),
                    "blue" => color = Some([0, 0, 255]),
                    "green" => color = Some([0, 128, 0]),
                    "black" => color = Some([0, 0, 0]),
                    "magenta" => color = Some([255, 0, 255]),
                    "cyan" => color = Some([0, 255, 255]),
                    "yellow" => color = Some([255, 255, 0]),
                    "white" => color = Some([255, 255, 255]),
                    "h" | "hh" => toks.push(Tok::Elapsed('h')),
                    "m" | "mm" => toks.push(Tok::Elapsed('m')),
                    "s" | "ss" => toks.push(Tok::Elapsed('s')),
                    _ => {
                        // [$€-409]: a currency symbol with a locale.
                        if let Some(sym) = inner.strip_prefix('$') {
                            lit(&mut toks, sym.split('-').next().unwrap_or("").to_owned());
                        }
                    }
                }
                i = end + 1;
            }
            '0' | '#' | '?' => {
                toks.push(Tok::Digit(c));
                i += 1;
            }
            '.' => {
                toks.push(Tok::Point);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            '%' => {
                toks.push(Tok::Percent);
                lit(&mut toks, String::new());
                i += 1;
            }
            'E' | 'e' if matches!(chars.get(i + 1), Some('+') | Some('-')) => {
                toks.push(Tok::Exp(chars[i + 1] == '+'));
                i += 2;
            }
            '@' => {
                toks.push(Tok::At);
                i += 1;
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => {
                let lc = c.to_ascii_lowercase();
                let mut s = String::new();
                while i < chars.len() && chars[i].to_ascii_lowercase() == lc {
                    s.push(lc);
                    i += 1;
                }
                toks.push(Tok::Date(s));
            }
            other => {
                lit(&mut toks, other.to_string());
                i += 1;
            }
        }
    }
    // Drop the empty literal a percent sign pushed to end a run.
    toks.retain(|t| !matches!(t, Tok::Lit(s) if s.is_empty()));
    (toks, color)
}

/// Digits of `x` rounded half away from zero to `decimals` places, working from the
/// 15 significant digits Excel keeps (so 2.675 is 2.675, not 2.67499999…).
pub fn fixed(x: f64, decimals: usize) -> String {
    let sci = format!("{:.14e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mant
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
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
    let mut int_len = point.max(0) as usize;
    if round_digit >= 5 {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, 1);
                int_len += 1;
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
    while kept.len() < int_len + decimals {
        kept.insert(0, 0);
    }
    let split = kept.len() - decimals;
    let int: String = kept[..split].iter().map(|d| (b'0' + d) as char).collect();
    let int = int.trim_start_matches('0');
    let mut out = if int.is_empty() {
        "0".to_string()
    } else {
        int.to_string()
    };
    if decimals > 0 {
        out.push('.');
        out.extend(kept[split..].iter().map(|d| (b'0' + d) as char));
    }
    out
}
/// Round to `decimals` places the way Excel's ROUND does: on the 15-significant-digit
/// decimal the value displays as, half away from zero.
pub fn round_decimal(x: f64, decimals: i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let shown = x;
    if decimals >= 0 {
        let s = fixed(shown, decimals.min(30) as usize);
        let v: f64 = s.parse().unwrap_or(shown);
        if x < 0.0 {
            -v
        } else {
            v
        }
    } else {
        let scale = cw_determinism::math::pow(10.0, f64::from(-decimals));
        let v: f64 = fixed(shown.abs() / scale, 0).parse::<f64>().unwrap_or(0.0) * scale;
        if x < 0.0 {
            -v
        } else {
            v
        }
    }
}
fn group(int: &str) -> String {
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
fn format_number(x: f64, toks: &[Tok]) -> String {
    if toks.iter().any(|t| matches!(t, Tok::General)) {
        let mut out = String::new();
        for t in toks {
            match t {
                Tok::General => out.push_str(&general(x)),
                Tok::Lit(s) => out.push_str(s),
                _ => {}
            }
        }
        return out;
    }
    let percent = toks.iter().filter(|t| matches!(t, Tok::Percent)).count() as i32;
    let mut v = x * cw_determinism::math::pow(100.0, f64::from(percent));
    // Placeholder layout: integer part, fraction part, exponent.
    let first_digit = toks.iter().position(|t| matches!(t, Tok::Digit(_)));
    let Some(first_digit) = first_digit else {
        // No placeholders: the section is literal text (a zero section like "-").
        return toks
            .iter()
            .map(|t| match t {
                Tok::Lit(s) => s.clone(),
                Tok::Percent => "%".into(),
                _ => String::new(),
            })
            .collect();
    };
    let point = toks.iter().position(|t| matches!(t, Tok::Point));
    let exp = toks.iter().position(|t| matches!(t, Tok::Exp(_)));
    let last_digit = toks
        .iter()
        .rposition(|t| matches!(t, Tok::Digit(_)))
        .unwrap_or(first_digit);
    // The integer part runs to the point or exponent, or through the last placeholder
    // and any commas right after it (which scale).
    let int_end = point.or(exp).unwrap_or_else(|| {
        let mut e = last_digit + 1;
        while e < toks.len() && matches!(toks[e], Tok::Comma) {
            e += 1;
        }
        e
    });
    // Commas after the last integer digit scale by a thousand each.
    let mut scale = 0;
    let mut k = int_end;
    while k > 0 && matches!(toks[k - 1], Tok::Comma) {
        scale += 1;
        k -= 1;
    }
    let grouping = toks[first_digit..k].iter().any(|t| matches!(t, Tok::Comma));
    v /= cw_determinism::math::pow(1000.0, f64::from(scale));
    let frac_places: Vec<char> = match point {
        Some(p) => toks[p + 1..exp.unwrap_or(toks.len())]
            .iter()
            .filter_map(|t| match t {
                Tok::Digit(c) => Some(*c),
                _ => None,
            })
            .collect(),
        None => vec![],
    };
    let mut exponent_text = String::new();
    if let Some(e) = exp {
        let plus = matches!(toks[e], Tok::Exp(true));
        let int_places = toks[first_digit..int_end]
            .iter()
            .filter(|t| matches!(t, Tok::Digit(_)))
            .count()
            .max(1) as i32;
        let exp_digits = toks[e + 1..]
            .iter()
            .filter(|t| matches!(t, Tok::Digit(_)))
            .count()
            .max(1);
        let mut n = if v == 0.0 {
            0
        } else {
            cw_determinism::math::log10(v.abs()).floor() as i32
        };
        // Engineering-style codes (##0.0E+0) keep the exponent a multiple of the width.
        if int_places > 1
            && toks[first_digit..int_end]
                .iter()
                .any(|t| matches!(t, Tok::Digit('#')))
        {
            n = n.div_euclid(int_places) * int_places;
        } else {
            n -= int_places - 1;
        }
        v /= cw_determinism::math::pow(10.0, f64::from(n));
        let mant = fixed(v, frac_places.len());
        if mant.len() > 1 && mant.starts_with("10") && int_places == 1 {
            v /= 10.0;
            n += 1;
        }
        exponent_text = format!(
            "E{}{:0width$}",
            if n < 0 {
                "-"
            } else if plus {
                "+"
            } else {
                ""
            },
            n.abs(),
            width = exp_digits
        );
    }
    let text = fixed(v, frac_places.len());
    let (int_digits, frac_digits) = text.split_once('.').unwrap_or((&text, ""));
    // Trailing '#' fraction places drop zeros; '?' turns them into spaces.
    let mut frac: Vec<char> = frac_digits.chars().collect();
    for (i, p) in frac_places.iter().enumerate().rev() {
        if frac[i] != '0' || *p == '0' {
            break;
        }
        frac[i] = if *p == '?' { ' ' } else { '\0' };
    }
    let frac: String = frac.into_iter().filter(|c| *c != '\0').collect();
    // Integer placeholders are filled right to left; spare digits go on the left.
    let int_toks = &toks[first_digit..int_end];
    let min_int = int_toks
        .iter()
        .filter(|t| matches!(t, Tok::Digit('0')))
        .count();
    let mut digits: String = if int_digits == "0" && min_int == 0 {
        String::new()
    } else {
        int_digits.to_owned()
    };
    while digits.len() < min_int {
        digits.insert(0, '0');
    }
    if grouping {
        digits = group(&digits);
    }
    let mut out = String::new();
    for t in &toks[..first_digit] {
        if let Tok::Lit(s) = t {
            out.push_str(s);
        }
    }
    // Literals inside the integer part (a phone-number code) stay in place.
    let inner_literals = int_toks.iter().any(|t| matches!(t, Tok::Lit(_)));
    if inner_literals && !grouping {
        let slots = int_toks
            .iter()
            .filter(|t| matches!(t, Tok::Digit(_)))
            .count();
        let mut ds: Vec<char> = digits.chars().collect();
        while ds.len() < slots {
            ds.insert(0, ' ');
        }
        let lead = ds.len() - slots;
        let mut body = ds[..lead].iter().collect::<String>();
        let mut di = lead;
        for t in int_toks {
            match t {
                Tok::Digit(_) => {
                    let c = ds[di];
                    if c != ' ' {
                        body.push(c);
                    }
                    di += 1;
                }
                Tok::Lit(s) => body.push_str(s),
                _ => {}
            }
        }
        out.push_str(body.trim_start());
    } else {
        out.push_str(&digits);
    }
    if point.is_some() {
        if !frac.is_empty() || frac_places.iter().any(|c| *c == '0' || *c == '?') {
            out.push('.');
            out.push_str(&frac);
        } else if frac_places.is_empty() {
            out.push('.');
        }
    }
    out.push_str(&exponent_text);
    for t in &toks[(last_digit + 1).max(if point.is_none() && exp.is_none() {
        int_end
    } else {
        0
    })..]
    {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Percent => out.push('%'),
            _ => {}
        }
    }
    // Percent signs that precede the number (rare) still print.
    for t in &toks[..first_digit] {
        if matches!(t, Tok::Percent) {
            out.insert(0, '%');
        }
    }
    out
}
fn format_date(serial: f64, toks: &[Tok]) -> Option<String> {
    let (y, mo, d) = date::ymd(serial)?;
    let (h, mi, s) = date::hms(serial);
    let twelve = toks.iter().any(|t| matches!(t, Tok::AmPm(_)));
    let mut out = String::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::AmPm(full) => out.push_str(match (h < 12, full) {
                (true, true) => "AM",
                (false, true) => "PM",
                (true, false) => "A",
                (false, false) => "P",
            }),
            Tok::Elapsed(unit) => {
                let total_secs = (serial * 86_400.0).round() as i64;
                out.push_str(&match unit {
                    'h' => (total_secs / 3600).to_string(),
                    'm' => (total_secs / 60).to_string(),
                    _ => total_secs.to_string(),
                });
            }
            Tok::Date(code) => {
                // "m" is minutes after an hour or before a second.
                let prev = toks[..i]
                    .iter()
                    .rev()
                    .find(|t| matches!(t, Tok::Date(_) | Tok::Elapsed(_)));
                let next = toks[i + 1..].iter().find(|t| matches!(t, Tok::Date(_)));
                let minute = code.starts_with('m')
                    && code.len() <= 2
                    && (matches!(prev, Some(Tok::Date(p)) if p.starts_with('h'))
                        || matches!(prev, Some(Tok::Elapsed('h')))
                        || matches!(next, Some(Tok::Date(n)) if n.starts_with('s')));
                let hour12 = if h % 12 == 0 { 12 } else { h % 12 };
                let text = match (code.as_str(), minute) {
                    ("yy", _) | ("y", _) => format!("{:02}", y.rem_euclid(100)),
                    (c, _) if c.starts_with('y') => format!("{y:04}"),
                    ("m", true) => mi.to_string(),
                    ("mm", true) => format!("{mi:02}"),
                    ("m", false) => mo.to_string(),
                    ("mm", false) => format!("{mo:02}"),
                    ("mmm", _) => date::MONTHS[(mo - 1) as usize][..3].to_owned(),
                    ("mmmmm", _) => date::MONTHS[(mo - 1) as usize][..1].to_owned(),
                    (c, _) if c.starts_with('m') => date::MONTHS[(mo - 1) as usize].to_owned(),
                    ("d", _) => d.to_string(),
                    ("dd", _) => format!("{d:02}"),
                    ("ddd", _) => date::WEEKDAYS[date::weekday(serial) as usize][..3].to_owned(),
                    (c, _) if c.starts_with('d') => {
                        date::WEEKDAYS[date::weekday(serial) as usize].to_owned()
                    }
                    ("h", _) => (if twelve { hour12 } else { h }).to_string(),
                    (c, _) if c.starts_with('h') => {
                        format!("{:02}", if twelve { hour12 } else { h })
                    }
                    ("s", _) => s.to_string(),
                    (c, _) if c.starts_with('s') => format!("{s:02}"),
                    _ => String::new(),
                };
                out.push_str(&text);
            }
            Tok::Point => {
                // Fractional seconds: ".0", ".00".
                let places = toks[i + 1..]
                    .iter()
                    .take_while(|t| matches!(t, Tok::Digit('0')))
                    .count();
                if places > 0 {
                    let ms = date::millis(serial);
                    let frac = format!("{:03}", ms);
                    out.push('.');
                    out.push_str(&frac[..places.min(3)]);
                } else {
                    out.push('.');
                }
            }
            Tok::Digit(_) => {}
            Tok::Comma => out.push(','),
            Tok::Percent => out.push('%'),
            _ => {}
        }
    }
    Some(out)
}
fn is_date(toks: &[Tok]) -> bool {
    toks.iter()
        .any(|t| matches!(t, Tok::Date(_) | Tok::AmPm(_) | Tok::Elapsed(_)))
}
/// Whether a format code renders numbers as dates or times.
pub fn is_date_format(code: &str) -> bool {
    sections(code)
        .first()
        .is_some_and(|s| is_date(&tokenize(s).0))
}

/// Render a value through a format code.
pub fn format(v: &Value, code: &str) -> Formatted {
    let plain = |text: String| Formatted { text, color: None };
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        return plain(v.display());
    }
    let secs = sections(code);
    match v {
        Value::Number(x) => {
            let x = *x;
            let (section, negated) = match secs.len() {
                1 => (&secs[0], false),
                2 => {
                    if x < 0.0 {
                        (&secs[1], true)
                    } else {
                        (&secs[0], false)
                    }
                }
                _ => {
                    if x < 0.0 {
                        (&secs[1], true)
                    } else if x == 0.0 {
                        (&secs[2], false)
                    } else {
                        (&secs[0], false)
                    }
                }
            };
            let (toks, color) = tokenize(section);
            if toks.iter().any(|t| matches!(t, Tok::At))
                && !toks
                    .iter()
                    .any(|t| matches!(t, Tok::Digit(_) | Tok::General))
            {
                return Formatted {
                    text: v.display(),
                    color,
                };
            }
            if is_date(&toks) {
                return match format_date(x, &toks) {
                    Some(text) => Formatted { text, color },
                    // Out of Excel's date range: shown as hashes.
                    None => Formatted {
                        text: "#".repeat(10),
                        color,
                    },
                };
            }
            let mut text = format_number(x.abs(), &toks);
            // A single section keeps the sign; a dedicated negative section spells its own.
            if x < 0.0 && !negated {
                text.insert(0, '-');
            }
            Formatted { text, color }
        }
        Value::Text(t) => {
            let section = if secs.len() >= 4 {
                Some(&secs[3])
            } else {
                secs.iter().find(|s| s.contains('@'))
            };
            match section {
                Some(s) => {
                    let (toks, color) = tokenize(s);
                    let mut out = String::new();
                    for tok in &toks {
                        match tok {
                            Tok::At => out.push_str(t),
                            Tok::Lit(s) => out.push_str(s),
                            _ => {}
                        }
                    }
                    Formatted { text: out, color }
                }
                None => plain(t.clone()),
            }
        }
        other => plain(other.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(x: f64, code: &str) -> String {
        format(&Value::Number(x), code).text
    }
    #[test]
    fn number_codes_render_like_excel() {
        assert_eq!(f(1234.5, "#,##0.00"), "1,234.50");
        assert_eq!(f(1234.5, "0"), "1235");
        assert_eq!(f(-1234.5, "#,##0.00"), "-1,234.50");
        assert_eq!(f(0.125, "0.0%"), "12.5%");
        assert_eq!(f(2.675, "0.00"), "2.68");
        assert_eq!(f(0.5, "#.##"), ".5");
        assert_eq!(f(1234.5, "$#,##0.00"), "$1,234.50");
        assert_eq!(f(-5.0, "$#,##0.00;[Red]($#,##0.00)"), "($5.00)");
        assert_eq!(
            format(&Value::Number(-5.0), "0;[Red]-0").color,
            Some([255, 0, 0])
        );
        assert_eq!(f(12345.0, "0.00E+00"), "1.23E+04");
        assert_eq!(f(1500000.0, "#,##0,,\"M\""), "2M");
        assert_eq!(f(5551234.0, "000-0000"), "555-1234");
        assert_eq!(f(7.0, "000"), "007");
        assert_eq!(f(0.0, "#,##0;-#,##0;\"-\""), "-");
        assert_eq!(f(3.0, "General"), "3");
    }
    #[test]
    fn date_codes_render_like_excel() {
        let d = 46283.5625; // 2026-09-18 13:30
        assert_eq!(f(d, "m/d/yyyy"), "9/18/2026");
        assert_eq!(f(d, "yyyy-mm-dd"), "2026-09-18");
        assert_eq!(f(d, "d-mmm-yy"), "18-Sep-26");
        assert_eq!(f(d, "dddd, mmmm d, yyyy"), "Friday, September 18, 2026");
        assert_eq!(f(d, "h:mm AM/PM"), "1:30 PM");
        assert_eq!(f(d, "hh:mm:ss"), "13:30:00");
        assert_eq!(f(1.5, "[h]:mm"), "36:00");
        assert!(is_date_format("mmm-yy"));
        assert!(!is_date_format("0.00"));
    }
    #[test]
    fn text_sections_and_rounding() {
        assert_eq!(
            format(&Value::Text("x".into()), "0;0;0;\"<\"@\">\"").text,
            "<x>"
        );
        assert_eq!(format(&Value::Text("x".into()), "0.00").text, "x");
        assert_eq!(round_decimal(2.675, 2), 2.68);
        assert_eq!(round_decimal(-2.5, 0), -3.0);
        assert_eq!(round_decimal(1234.5, -2), 1200.0);
        assert_eq!(fixed(999.996, 2), "1000.00");
    }
}
