//! The line-oriented text utilities, flag-accurate.
//!
//! Every option listed in `docs/shell.md` is implemented here; every option that is
//! not listed is rejected by name with GNU's own wording and status 2. Nothing in
//! this module quietly ignores a flag, which is the single failure mode that makes a
//! simulated shell untrustworthy.
use crate::shell::{
    flag, invalid_option, missing_argument, options, read_bytes, read_text, unrecognized_option,
    usage_line, value, Fail,
};
use crate::{Computer, ShellHost};

/// Reads every operand in turn, or standard input when there are none.
/// The result pairs each label with its text so `-v`/`-H` style headers stay honest.
fn sources(
    c: &Computer,
    cmd: &str,
    operands: &[String],
    input: &str,
) -> Result<Vec<(String, String)>, Fail> {
    if operands.is_empty() {
        return Ok(vec![(String::from("-"), input.to_string())]);
    }
    let mut out = Vec::with_capacity(operands.len());
    for name in operands {
        out.push((name.clone(), read_text(c, cmd, name, input)?));
    }
    Ok(out)
}
/// Lines without their terminators, plus whether the text ended with a newline.
fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}
fn joined(lines: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(l.as_ref());
        out.push('\n');
    }
    out
}
/// Parses a count operand, naming the flag when it is not a number.
fn count(cmd: &str, flagname: &str, raw: &str) -> Result<usize, Fail> {
    raw.parse().map_err(|_| {
        Fail::usage(format!(
            "{cmd}: invalid number of {flagname}: '{raw}'\n{}",
            usage_line(cmd)
        ))
    })
}
/// `-c`/`-b` sizes accept GNU's suffixes.
fn size(cmd: &str, raw: &str) -> Result<usize, Fail> {
    let (digits, factor) = match raw.chars().last() {
        Some('b') => (&raw[..raw.len() - 1], 512usize),
        Some('K' | 'k') => (&raw[..raw.len() - 1], 1024),
        Some('M' | 'm') => (&raw[..raw.len() - 1], 1024 * 1024),
        Some('G' | 'g') => (&raw[..raw.len() - 1], 1024 * 1024 * 1024),
        _ => (raw, 1),
    };
    digits
        .parse::<usize>()
        .map(|n| n * factor)
        .map_err(|_| Fail::usage(format!("{cmd}: invalid number of bytes: '{raw}'")))
}

// ---------------------------------------------------------------- cut

fn parse_ranges(cmd: &str, spec: &str) -> Result<Vec<(usize, usize)>, Fail> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        if part.is_empty() {
            continue;
        }
        let range = match part.split_once('-') {
            None => {
                let n = part
                    .parse::<usize>()
                    .map_err(|_| Fail::usage(format!("{cmd}: invalid field value '{part}'")))?;
                (n, n)
            }
            Some(("", to)) => (
                1,
                to.parse()
                    .map_err(|_| Fail::usage(format!("{cmd}: invalid field value '{to}'")))?,
            ),
            Some((from, "")) => (
                from.parse()
                    .map_err(|_| Fail::usage(format!("{cmd}: invalid field value '{from}'")))?,
                usize::MAX,
            ),
            Some((from, to)) => (
                from.parse()
                    .map_err(|_| Fail::usage(format!("{cmd}: invalid field value '{from}'")))?,
                to.parse()
                    .map_err(|_| Fail::usage(format!("{cmd}: invalid field value '{to}'")))?,
            ),
        };
        if range.0 == 0 {
            return Err(Fail::usage(format!(
                "{cmd}: fields and positions are numbered from 1"
            )));
        }
        if range.0 > range.1 {
            return Err(Fail::usage(format!(
                "{cmd}: invalid decreasing range '{part}'"
            )));
        }
        out.push(range);
    }
    if out.is_empty() {
        return Err(Fail::usage(format!("{cmd}: invalid empty range list")));
    }
    out.sort_unstable();
    Ok(out)
}
fn in_ranges(ranges: &[(usize, usize)], n: usize, complement: bool) -> bool {
    ranges.iter().any(|(a, b)| n >= *a && n <= *b) != complement
}
fn cut(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "cut",
        args,
        "snC",
        "bcfdO",
        &[
            ("bytes", 'b'),
            ("characters", 'c'),
            ("fields", 'f'),
            ("delimiter", 'd'),
            ("only-delimited", 's'),
            ("complement", 'C'),
            ("output-delimiter", 'O'),
        ],
    )?;
    let complement = flag(&opts, 'C');
    let chosen: Vec<char> = "bcf"
        .chars()
        .filter(|k| value(&opts, *k).is_some())
        .collect();
    if chosen.len() != 1 {
        return Err(Fail::usage(format!(
            "cut: you must specify exactly one of -b, -c or -f\n{}",
            usage_line("cut")
        )));
    }
    let mode = chosen[0];
    let ranges = parse_ranges("cut", value(&opts, mode).unwrap())?;
    let delimiter = match value(&opts, 'd') {
        Some(d) => {
            let chars: Vec<char> = unescape(d).chars().collect();
            match chars.len() {
                1 => chars[0],
                _ => return Err(Fail::usage("cut: the delimiter must be a single character")),
            }
        }
        None => '\t',
    };
    if mode != 'f' && (value(&opts, 'd').is_some() || flag(&opts, 's')) {
        return Err(Fail::usage(
            "cut: an input delimiter may be specified only when operating on fields",
        ));
    }
    let output_delimiter = value(&opts, 'O').map(unescape);
    let mut out = String::new();
    for (_, text) in sources(c, "cut", &operands, input)? {
        for line in lines_of(&text) {
            match mode {
                'f' => {
                    if !line.contains(delimiter) {
                        if !flag(&opts, 's') {
                            out.push_str(line);
                            out.push('\n');
                        }
                        continue;
                    }
                    let od = output_delimiter
                        .clone()
                        .unwrap_or_else(|| delimiter.to_string());
                    let kept: Vec<&str> = line
                        .split(delimiter)
                        .enumerate()
                        .filter(|(i, _)| in_ranges(&ranges, i + 1, complement))
                        .map(|(_, f)| f)
                        .collect();
                    out.push_str(&kept.join(&od));
                    out.push('\n');
                }
                'c' => {
                    let od = output_delimiter.clone();
                    let kept: Vec<String> = line
                        .chars()
                        .enumerate()
                        .filter(|(i, _)| in_ranges(&ranges, i + 1, complement))
                        .map(|(_, ch)| ch.to_string())
                        .collect();
                    out.push_str(&match od {
                        Some(d) => group_runs(&ranges, line.chars().count(), complement, &kept, &d),
                        None => kept.concat(),
                    });
                    out.push('\n');
                }
                _ => {
                    let bytes = line.as_bytes();
                    let kept: Vec<u8> = bytes
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| in_ranges(&ranges, i + 1, complement))
                        .map(|(_, b)| *b)
                        .collect();
                    out.push_str(&String::from_utf8_lossy(&kept));
                    out.push('\n');
                }
            }
        }
    }
    Ok(out)
}
/// `--output-delimiter` with `-c` joins each selected *range*, not each character.
fn group_runs(
    ranges: &[(usize, usize)],
    len: usize,
    complement: bool,
    kept: &[String],
    delimiter: &str,
) -> String {
    let mut groups: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous: Option<usize> = None;
    let mut k = 0;
    for i in 1..=len {
        if !in_ranges(ranges, i, complement) {
            continue;
        }
        if previous.is_some_and(|p| i != p + 1) {
            groups.push(std::mem::take(&mut current));
        }
        current.push_str(&kept[k]);
        k += 1;
        previous = Some(i);
    }
    if !current.is_empty() || groups.is_empty() {
        groups.push(current);
    }
    groups.join(delimiter)
}
/// Turns `\t`, `\n` and friends in a command-line operand into their characters.
pub(crate) fn unescape(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        match b[i] {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            '\\' => out.push('\\'),
            other => {
                out.push('\\');
                out.push(other);
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- sort

#[derive(Clone, Default)]
struct KeyOpts {
    numeric: bool,
    general: bool,
    human: bool,
    month: bool,
    version: bool,
    fold: bool,
    blanks: bool,
    dictionary: bool,
    ignore: bool,
    reverse: bool,
}
#[derive(Clone)]
struct Key {
    from_field: usize,
    from_char: usize,
    to_field: Option<usize>,
    to_char: Option<usize>,
    opts: KeyOpts,
}
/// Field boundaries. Without `-t`, a field carries the blanks that precede it, which
/// is what makes `-k2` and `-k2b` differ.
fn field_bounds(line: &str, sep: Option<char>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    match sep {
        Some(t) => {
            let mut start = 0;
            for (i, ch) in line.char_indices() {
                if ch == t {
                    out.push((start, i));
                    start = i + ch.len_utf8();
                }
            }
            out.push((start, line.len()));
        }
        None => {
            let b = line.as_bytes();
            let mut i = 0;
            while i < b.len() {
                let start = i;
                while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                    i += 1;
                }
                while i < b.len() && b[i] != b' ' && b[i] != b'\t' {
                    i += 1;
                }
                out.push((start, i));
            }
        }
    }
    out
}
fn key_text(line: &str, key: &Key, sep: Option<char>) -> String {
    let fields = field_bounds(line, sep);
    if key.from_field == 0 || key.from_field > fields.len() {
        return String::new();
    }
    let (mut start, field_end) = fields[key.from_field - 1];
    if key.opts.blanks {
        while start < field_end && matches!(line.as_bytes()[start], b' ' | b'\t') {
            start += 1;
        }
    }
    let start = advance_chars(line, start, key.from_char.saturating_sub(1)).min(line.len());
    let end = match key.to_field {
        None => line.len(),
        Some(f) if f == 0 || f > fields.len() => line.len(),
        Some(f) => {
            let (mut fs, fe) = fields[f - 1];
            if key.opts.blanks {
                while fs < fe && matches!(line.as_bytes()[fs], b' ' | b'\t') {
                    fs += 1;
                }
            }
            match key.to_char {
                None | Some(0) => fe,
                Some(ch) => advance_chars(line, fs, ch).min(fe),
            }
        }
    };
    line[start.min(end)..end].to_string()
}
fn advance_chars(line: &str, from: usize, n: usize) -> usize {
    let mut at = from;
    for _ in 0..n {
        match line[at..].chars().next() {
            Some(ch) => at += ch.len_utf8(),
            None => break,
        }
    }
    at
}
fn parse_key(spec: &str) -> Result<Key, Fail> {
    let bad = || Fail::usage(format!("sort: invalid key specification '{spec}'"));
    let (start, end) = match spec.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (spec, None),
    };
    let split_opts = |s: &str| -> (String, String) {
        let idx = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
        (s[..idx].to_string(), s[idx..].to_string())
    };
    let (start_pos, start_opts) = split_opts(start);
    let (from_field, from_char) = match start_pos.split_once('.') {
        Some((f, c)) => (f.parse().map_err(|_| bad())?, c.parse().map_err(|_| bad())?),
        None => (start_pos.parse().map_err(|_| bad())?, 1usize),
    };
    let mut opts = KeyOpts::default();
    let mut apply = |letters: &str| -> Result<(), Fail> {
        for ch in letters.chars() {
            match ch {
                'n' => opts.numeric = true,
                'g' => opts.general = true,
                'h' => opts.human = true,
                'M' => opts.month = true,
                'V' => opts.version = true,
                'f' => opts.fold = true,
                'b' => opts.blanks = true,
                'd' => opts.dictionary = true,
                'i' => opts.ignore = true,
                'r' => opts.reverse = true,
                other => {
                    return Err(Fail::usage(format!(
                        "sort: unknown key modifier '{other}' in '{spec}'"
                    )))
                }
            }
        }
        Ok(())
    };
    apply(&start_opts)?;
    let (to_field, to_char) = match end {
        None => (None, None),
        Some(e) => {
            let (pos, letters) = split_opts(e);
            apply(&letters)?;
            match pos.split_once('.') {
                Some((f, ch)) => (
                    Some(f.parse().map_err(|_| bad())?),
                    Some(ch.parse().map_err(|_| bad())?),
                ),
                None => (Some(pos.parse().map_err(|_| bad())?), None),
            }
        }
    };
    if from_field == 0 {
        return Err(bad());
    }
    Ok(Key {
        from_field,
        from_char,
        to_field,
        to_char,
        opts,
    })
}
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
fn month_rank(s: &str) -> usize {
    let t = s.trim_start().to_uppercase();
    MONTHS
        .iter()
        .position(|m| t.starts_with(m))
        .map_or(0, |i| i + 1)
}
fn human_value(s: &str) -> f64 {
    let t = s.trim_start();
    let digits: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    let n: f64 = digits.parse().unwrap_or(0.0);
    let suffix = t[digits.len()..].chars().next().unwrap_or(' ');
    let factor = match suffix {
        'K' | 'k' => 1024.0,
        'M' => 1024f64.powi(2),
        'G' => 1024f64.powi(3),
        'T' => 1024f64.powi(4),
        'P' => 1024f64.powi(5),
        'E' => 1024f64.powi(6),
        _ => 1.0,
    };
    n * factor
}
fn numeric_value(s: &str) -> f64 {
    let t = s.trim_start();
    let mut end = 0;
    let b: Vec<char> = t.chars().collect();
    if end < b.len() && (b[end] == '-' || b[end] == '+') {
        end += 1;
    }
    while end < b.len() && (b[end].is_ascii_digit() || b[end] == ',') {
        end += 1;
    }
    if end < b.len() && b[end] == '.' {
        end += 1;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
    }
    b[..end]
        .iter()
        .filter(|c| **c != ',')
        .collect::<String>()
        .parse()
        .unwrap_or(0.0)
}
/// GNU's version sort: digit runs compare numerically, everything else byte-wise.
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(p), Some(q)) => {
                if p.is_ascii_digit() && q.is_ascii_digit() {
                    let mut pa = String::new();
                    while x.peek().is_some_and(char::is_ascii_digit) {
                        pa.push(x.next().unwrap());
                    }
                    let mut qa = String::new();
                    while y.peek().is_some_and(char::is_ascii_digit) {
                        qa.push(y.next().unwrap());
                    }
                    let (pn, qn) = (
                        pa.trim_start_matches('0').to_string(),
                        qa.trim_start_matches('0').to_string(),
                    );
                    let order = pn.len().cmp(&qn.len()).then_with(|| pn.cmp(&qn));
                    if order != std::cmp::Ordering::Equal {
                        return order;
                    }
                } else {
                    x.next();
                    y.next();
                    let order = p.cmp(&q);
                    if order != std::cmp::Ordering::Equal {
                        return order;
                    }
                }
            }
        }
    }
}
fn prepare(text: &str, o: &KeyOpts) -> String {
    let mut s = text.to_string();
    if o.blanks {
        s = s.trim_start_matches([' ', '\t']).to_string();
    }
    if o.dictionary {
        s = s
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '\t')
            .collect();
    }
    if o.ignore {
        s = s.chars().filter(|c| !c.is_control()).collect();
    }
    if o.fold {
        s = s.to_uppercase();
    }
    s
}
fn compare_with(a: &str, b: &str, o: &KeyOpts) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (x, y) = (prepare(a, o), prepare(b, o));
    let order = if o.numeric {
        numeric_value(&x)
            .partial_cmp(&numeric_value(&y))
            .unwrap_or(Ordering::Equal)
    } else if o.general {
        x.trim()
            .parse::<f64>()
            .unwrap_or(f64::NEG_INFINITY)
            .partial_cmp(&y.trim().parse::<f64>().unwrap_or(f64::NEG_INFINITY))
            .unwrap_or(Ordering::Equal)
    } else if o.human {
        human_value(&x)
            .partial_cmp(&human_value(&y))
            .unwrap_or(Ordering::Equal)
    } else if o.month {
        month_rank(&x).cmp(&month_rank(&y))
    } else if o.version {
        version_cmp(&x, &y)
    } else {
        x.cmp(&y)
    };
    if o.reverse {
        order.reverse()
    } else {
        order
    }
}
fn sort(c: &mut Computer, args: &[String], input: &str, t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "sort",
        args,
        "nrugbfMVcshdiz",
        "kto",
        &[
            ("numeric-sort", 'n'),
            ("reverse", 'r'),
            ("unique", 'u'),
            ("general-numeric-sort", 'g'),
            ("human-numeric-sort", 'h'),
            ("ignore-leading-blanks", 'b'),
            ("ignore-case", 'f'),
            ("month-sort", 'M'),
            ("version-sort", 'V'),
            ("check", 'c'),
            ("stable", 's'),
            ("dictionary-order", 'd'),
            ("ignore-nonprinting", 'i'),
            ("zero-terminated", 'z'),
            ("key", 'k'),
            ("field-separator", 't'),
            ("output", 'o'),
        ],
    )?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(
            "sort: -z (NUL-terminated lines) is not modelled by this world",
        ));
    }
    let separator = match value(&opts, 't') {
        Some(raw) => {
            let chars: Vec<char> = unescape(raw).chars().collect();
            match chars.len() {
                1 => Some(chars[0]),
                _ => {
                    return Err(Fail::usage(
                        "sort: the field separator must be a single character",
                    ))
                }
            }
        }
        None => None,
    };
    let global = KeyOpts {
        numeric: flag(&opts, 'n'),
        general: flag(&opts, 'g'),
        human: flag(&opts, 'h'),
        month: flag(&opts, 'M'),
        version: flag(&opts, 'V'),
        fold: flag(&opts, 'f'),
        blanks: flag(&opts, 'b'),
        dictionary: flag(&opts, 'd'),
        ignore: flag(&opts, 'i'),
        reverse: false,
    };
    let mut keys: Vec<Key> = Vec::new();
    for (k, v) in &opts {
        if *k == 'k' {
            let mut key = parse_key(v)?;
            // A key with no type letters inherits the global ones.
            let bare = !(key.opts.numeric
                || key.opts.general
                || key.opts.human
                || key.opts.month
                || key.opts.version
                || key.opts.fold
                || key.opts.dictionary
                || key.opts.ignore);
            if bare {
                let reverse = key.opts.reverse;
                let blanks = key.opts.blanks;
                key.opts = global.clone();
                key.opts.reverse = reverse;
                key.opts.blanks |= blanks;
            }
            keys.push(key);
        }
    }
    let mut text = String::new();
    for (_, body) in sources(c, "sort", &operands, input)? {
        text.push_str(&body);
        if !body.is_empty() && !body.ends_with('\n') {
            text.push('\n');
        }
    }
    let lines: Vec<String> = lines_of(&text).iter().map(|s| (*s).to_string()).collect();
    let reverse = flag(&opts, 'r');
    let compare = |a: &String, b: &String| -> std::cmp::Ordering {
        let mut order = std::cmp::Ordering::Equal;
        for key in &keys {
            order = compare_with(
                &key_text(a, key, separator),
                &key_text(b, key, separator),
                &key.opts,
            );
            if order != std::cmp::Ordering::Equal {
                break;
            }
        }
        if order == std::cmp::Ordering::Equal && (keys.is_empty() || !flag(&opts, 's')) {
            order = compare_with(a, b, &global);
        }
        if reverse {
            order.reverse()
        } else {
            order
        }
    };
    if flag(&opts, 'c') {
        let label = operands.first().cloned().unwrap_or_else(|| "-".into());
        for (i, pair) in lines.windows(2).enumerate() {
            if compare(&pair[0], &pair[1]) == std::cmp::Ordering::Greater {
                return Err(Fail::new(
                    format!("sort: {label}:{}: disorder: {}", i + 2, pair[1]),
                    1,
                ));
            }
        }
        return Ok(String::new());
    }
    let mut sorted = lines;
    sorted.sort_by(compare);
    if flag(&opts, 'u') {
        sorted.dedup_by(|a, b| compare(a, b) == std::cmp::Ordering::Equal);
    }
    let out = joined(&sorted);
    match value(&opts, 'o') {
        Some(path) => {
            let resolved = c.resolve(path);
            c.vfs
                .write_as(&resolved, out.as_bytes(), &c.user, t)
                .map_err(|e| Fail::io("sort", path, &e))?;
            Ok(String::new())
        }
        None => Ok(out),
    }
}

// ---------------------------------------------------------------- uniq

fn uniq(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "uniq",
        args,
        "cduDi",
        "fsw",
        &[
            ("count", 'c'),
            ("repeated", 'd'),
            ("all-repeated", 'D'),
            ("unique", 'u'),
            ("ignore-case", 'i'),
            ("skip-fields", 'f'),
            ("skip-chars", 's'),
            ("check-chars", 'w'),
        ],
    )?;
    let skip_fields = match value(&opts, 'f') {
        Some(v) => count("uniq", "fields to skip", v)?,
        None => 0,
    };
    let skip_chars = match value(&opts, 's') {
        Some(v) => count("uniq", "bytes to skip", v)?,
        None => 0,
    };
    let width = match value(&opts, 'w') {
        Some(v) => Some(count("uniq", "bytes to compare", v)?),
        None => None,
    };
    let text = read_text(
        c,
        "uniq",
        operands.first().map_or("-", String::as_str),
        input,
    )?;
    let key = |line: &str| -> String {
        let bounds = field_bounds(line, None);
        let start = bounds
            .get(skip_fields)
            .map_or(line.len(), |(s, _)| *s)
            .min(line.len());
        // Skipped fields keep their leading blanks out of the comparison.
        let start = {
            let mut s = start;
            while s < line.len() && matches!(line.as_bytes()[s], b' ' | b'\t') && skip_fields > 0 {
                s += 1;
            }
            s
        };
        let start = advance_chars(line, start, skip_chars);
        let tail = &line[start.min(line.len())..];
        let body: String = match width {
            Some(w) => tail.chars().take(w).collect(),
            None => tail.to_string(),
        };
        if flag(&opts, 'i') {
            body.to_uppercase()
        } else {
            body
        }
    };
    let mut runs: Vec<(usize, String)> = Vec::new();
    for line in lines_of(&text) {
        match runs.last_mut() {
            Some(last) if key(&last.1) == key(line) => last.0 += 1,
            _ => runs.push((1, line.to_string())),
        }
    }
    let mut out = String::new();
    for (n, line) in runs {
        let show = if flag(&opts, 'd') || flag(&opts, 'D') {
            n > 1
        } else if flag(&opts, 'u') {
            n == 1
        } else {
            true
        };
        if !show {
            continue;
        }
        if flag(&opts, 'D') {
            for _ in 0..n {
                out.push_str(&line);
                out.push('\n');
            }
            continue;
        }
        if flag(&opts, 'c') {
            out.push_str(&format!("{n:>7} {line}\n"));
        } else {
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- head / tail

fn head_tail(c: &Computer, cmd: &str, args: &[String], input: &str) -> Result<String, Fail> {
    // `head -3` is the historical spelling of `head -n 3`, and is what people type.
    let rewritten: Vec<String> = {
        let mut out = Vec::with_capacity(args.len() + 1);
        let mut expecting_value = false;
        for arg in args {
            // `-n -1` and `-c -5` carry their own operand: only a bare `-3` is the
            // historical shorthand for `-n 3`.
            let shorthand = !expecting_value
                && arg
                    .strip_prefix('-')
                    .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
            expecting_value = matches!(arg.as_str(), "-n" | "-c");
            if shorthand {
                out.push("-n".to_owned());
                out.push(arg[1..].to_owned());
            } else {
                out.push(arg.clone());
            }
        }
        out
    };
    let (opts, operands) = options(
        cmd,
        &rewritten,
        "qvzf",
        "nc",
        &[
            ("lines", 'n'),
            ("bytes", 'c'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("verbose", 'v'),
            ("zero-terminated", 'z'),
            ("follow", 'f'),
        ],
    )?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(format!(
            "{cmd}: -z (NUL-terminated lines) is not modelled by this world"
        )));
    }
    if flag(&opts, 'f') {
        return Err(Fail::usage(
            "tail: -f (follow) has nothing to follow: this world's files change only \
             when a command in this same shell writes them, so a follow would never wake",
        ));
    }
    let raw = value(&opts, 'n').map(str::to_string);
    let bytes = value(&opts, 'c').map(str::to_string);
    if raw.is_some() && bytes.is_some() {
        return Err(Fail::usage(format!("{cmd}: cannot use -n and -c together")));
    }
    let spec = raw.clone().or_else(|| bytes.clone());
    // `+N` counts from the start; a bare `-N` is GNU's "all but the last N" for head
    // and "the last N" for tail.
    let (from_start, all_but, amount) = match &spec {
        Some(v) => match (v.strip_prefix('+'), v.strip_prefix('-')) {
            (Some(rest), _) => (true, false, count(cmd, "lines", rest)?),
            (_, Some(rest)) => (false, cmd == "head", count(cmd, "lines", rest)?),
            _ => (false, false, count(cmd, "lines", v)?),
        },
        None => (false, false, 10),
    };
    let by_bytes = bytes.is_some();
    let items = sources(c, cmd, &operands, input)?;
    let label = !flag(&opts, 'q') && (flag(&opts, 'v') || items.len() > 1);
    let mut out = String::new();
    for (i, (name, text)) in items.iter().enumerate() {
        if label {
            if i > 0 {
                out.push('\n');
            }
            let shown = if name == "-" { "standard input" } else { name };
            out.push_str(&format!("==> {shown} <==\n"));
        }
        if by_bytes {
            let b = text.as_bytes();
            let slice: &[u8] = if cmd == "head" {
                if all_but {
                    &b[..b.len().saturating_sub(amount)]
                } else {
                    &b[..amount.min(b.len())]
                }
            } else if from_start {
                &b[amount.saturating_sub(1).min(b.len())..]
            } else {
                &b[b.len().saturating_sub(amount)..]
            };
            out.push_str(&String::from_utf8_lossy(slice));
            continue;
        }
        let lines = lines_of(text);
        let chosen: Vec<&str> = if cmd == "head" {
            let take = if all_but {
                lines.len().saturating_sub(amount)
            } else {
                amount
            };
            lines.iter().take(take).copied().collect()
        } else if from_start {
            lines
                .iter()
                .skip(amount.saturating_sub(1))
                .copied()
                .collect()
        } else {
            lines
                .iter()
                .skip(lines.len().saturating_sub(amount))
                .copied()
                .collect()
        };
        out.push_str(&joined(&chosen));
    }
    Ok(out)
}

// ---------------------------------------------------------------- wc

fn wc(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "wc",
        args,
        "lwcmL",
        "",
        &[
            ("lines", 'l'),
            ("words", 'w'),
            ("bytes", 'c'),
            ("chars", 'm'),
            ("max-line-length", 'L'),
        ],
    )?;
    let chosen: Vec<char> = "lwcmL"
        .chars()
        .filter(|f| flag(&opts, *f))
        .collect::<Vec<_>>();
    let chosen = if chosen.is_empty() {
        vec!['l', 'w', 'c']
    } else {
        chosen
    };
    let items = sources(c, "wc", &operands, input)?;
    let mut out = String::new();
    let mut totals = [0usize; 5];
    for (name, text) in &items {
        let counts = [
            text.bytes().filter(|b| *b == b'\n').count(),
            text.split_whitespace().count(),
            text.len(),
            text.chars().count(),
            text.lines().map(|l| l.chars().count()).max().unwrap_or(0),
        ];
        for (i, v) in counts.iter().enumerate() {
            if i == 4 {
                totals[i] = totals[i].max(*v);
            } else {
                totals[i] += v;
            }
        }
        let row: Vec<String> = chosen
            .iter()
            .map(|f| counts["lwcmL".find(*f).unwrap()].to_string())
            .collect();
        out.push_str(&row.join(" "));
        if !operands.is_empty() {
            out.push(' ');
            out.push_str(name);
        }
        out.push('\n');
    }
    if items.len() > 1 {
        let row: Vec<String> = chosen
            .iter()
            .map(|f| totals["lwcmL".find(*f).unwrap()].to_string())
            .collect();
        out.push_str(&format!("{} total\n", row.join(" ")));
    }
    Ok(out)
}

// ---------------------------------------------------------------- tr

/// Expands a `tr` set: ranges, `\` escapes, `[:class:]`, `[c*n]` and `[c*]`.
fn tr_set(cmd: &str, spec: &str, target_len: Option<usize>) -> Result<Vec<char>, Fail> {
    let b: Vec<char> = spec.chars().collect();
    let mut out: Vec<char> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '[' {
            let rest: String = b[i..].iter().collect();
            if let Some(end) = rest.find(":]") {
                let name = &rest[2..end];
                if !rest.starts_with("[:") {
                    // fall through to literal handling
                } else {
                    let class: Vec<char> = match name {
                        "alpha" => (b'A'..=b'Z').chain(b'a'..=b'z').map(char::from).collect(),
                        "digit" => (b'0'..=b'9').map(char::from).collect(),
                        "alnum" => (b'0'..=b'9')
                            .chain(b'A'..=b'Z')
                            .chain(b'a'..=b'z')
                            .map(char::from)
                            .collect(),
                        "lower" => (b'a'..=b'z').map(char::from).collect(),
                        "upper" => (b'A'..=b'Z').map(char::from).collect(),
                        "space" => vec![' ', '\t', '\n', '\u{b}', '\u{c}', '\r'],
                        "blank" => vec![' ', '\t'],
                        "punct" => (33u8..127)
                            .map(char::from)
                            .filter(|c| !c.is_ascii_alphanumeric())
                            .collect(),
                        "print" => (32u8..127).map(char::from).collect(),
                        "graph" => (33u8..127).map(char::from).collect(),
                        "cntrl" => (0u8..32).map(char::from).chain([char::from(127)]).collect(),
                        "xdigit" => (b'0'..=b'9')
                            .chain(b'A'..=b'F')
                            .chain(b'a'..=b'f')
                            .map(char::from)
                            .collect(),
                        other => {
                            return Err(Fail::usage(format!(
                                "{cmd}: invalid character class '{other}'"
                            )))
                        }
                    };
                    out.extend(class);
                    i += end + 2;
                    continue;
                }
            }
            // `[c*n]` repeats c n times; `[c*]` pads to the other set's length.
            if let Some(end) = rest.find(']') {
                let body = &rest[1..end];
                if let Some((ch, times)) = body.split_once('*') {
                    let ch = unescape(ch).chars().next().unwrap_or('?');
                    let n = if times.is_empty() {
                        target_len.unwrap_or(0).saturating_sub(out.len())
                    } else if let Some(octal) = times.strip_prefix('0') {
                        usize::from_str_radix(octal, 8).unwrap_or(0)
                    } else {
                        times.parse().unwrap_or(0)
                    };
                    out.extend(std::iter::repeat_n(ch, n));
                    i += end + 1;
                    continue;
                }
            }
        }
        if b[i] == '\\' && i + 1 < b.len() {
            let text = unescape(&b[i..i + 2].iter().collect::<String>());
            out.extend(text.chars());
            i += 2;
            continue;
        }
        if i + 2 < b.len() && b[i + 1] == '-' && b[i + 2] != ']' {
            let (from, to) = (b[i], b[i + 2]);
            if (from as u32) > (to as u32) {
                return Err(Fail::usage(format!(
                    "{cmd}: range-endpoints of '{from}-{to}' are in reverse collating order"
                )));
            }
            for code in (from as u32)..=(to as u32) {
                if let Some(ch) = char::from_u32(code) {
                    out.push(ch);
                }
            }
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    Ok(out)
}
fn tr(args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "tr",
        args,
        "dsct",
        "",
        &[
            ("delete", 'd'),
            ("squeeze-repeats", 's'),
            ("complement", 'c'),
            ("truncate-set1", 't'),
        ],
    )?;
    let complement = flag(&opts, 'c');
    let delete = flag(&opts, 'd');
    let squeeze = flag(&opts, 's');
    if operands.is_empty() {
        return Err(Fail::usage(format!(
            "tr: missing operand\n{}",
            usage_line("tr")
        )));
    }
    if operands.len() > 2 {
        return Err(Fail::usage(format!(
            "tr: extra operand '{}'\n{}",
            operands[2],
            usage_line("tr")
        )));
    }
    if !delete && operands.len() < 2 && !squeeze {
        return Err(Fail::usage(format!(
            "tr: missing operand after '{}'\n{}",
            operands[0],
            usage_line("tr")
        )));
    }
    let set1 = tr_set("tr", &operands[0], None)?;
    let set2 = match operands.get(1) {
        Some(s) => tr_set("tr", s, Some(set1.len()))?,
        None => Vec::new(),
    };
    let member = |sets: &[char], ch: char| sets.contains(&ch) != complement;
    let mut out = String::new();
    if delete {
        let kept: String = input.chars().filter(|ch| !member(&set1, *ch)).collect();
        if squeeze && !set2.is_empty() {
            return Ok(squeeze_runs(&kept, &set2, false));
        }
        return Ok(kept);
    }
    if set2.is_empty() {
        // `tr -s SET1` squeezes without translating.
        return Ok(squeeze_runs(input, &set1, complement));
    }
    let mut set2 = set2;
    if flag(&opts, 't') {
        set2.truncate(set1.len());
    }
    let last = *set2.last().unwrap();
    for ch in input.chars() {
        if member(&set1, ch) {
            let mapped = if complement {
                last
            } else {
                set1.iter()
                    .position(|c| *c == ch)
                    .and_then(|i| set2.get(i).copied())
                    .unwrap_or(last)
            };
            out.push(mapped);
        } else {
            out.push(ch);
        }
    }
    if squeeze {
        out = squeeze_runs(&out, &set2, false);
    }
    Ok(out)
}
fn squeeze_runs(text: &str, set: &[char], complement: bool) -> String {
    let mut out = String::new();
    let mut previous: Option<char> = None;
    for ch in text.chars() {
        let in_set = set.contains(&ch) != complement;
        if in_set && previous == Some(ch) {
            continue;
        }
        out.push(ch);
        previous = Some(ch);
    }
    out
}

// ---------------------------------------------------------------- paste / join / comm

fn paste(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "paste",
        args,
        "sz",
        "d",
        &[
            ("serial", 's'),
            ("delimiters", 'd'),
            ("zero-terminated", 'z'),
        ],
    )?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(
            "paste: -z (NUL-terminated lines) is not modelled by this world",
        ));
    }
    let delims: Vec<char> = match value(&opts, 'd') {
        Some(d) => {
            let expanded = unescape(d);
            if expanded.is_empty() {
                vec!['\0']
            } else {
                expanded.chars().collect()
            }
        }
        None => vec!['\t'],
    };
    let items = sources(c, "paste", &operands, input)?;
    let mut out = String::new();
    if flag(&opts, 's') {
        for (_, text) in &items {
            let lines = lines_of(text);
            let mut row = String::new();
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    row.push(delims[(i - 1) % delims.len()]);
                }
                row.push_str(line);
            }
            out.push_str(&row);
            out.push('\n');
        }
        return Ok(out);
    }
    let columns: Vec<Vec<&str>> = items.iter().map(|(_, t)| lines_of(t)).collect();
    let rows = columns.iter().map(Vec::len).max().unwrap_or(0);
    for r in 0..rows {
        let mut row = String::new();
        for (i, column) in columns.iter().enumerate() {
            if i > 0 {
                row.push(delims[(i - 1) % delims.len()]);
            }
            row.push_str(column.get(r).copied().unwrap_or(""));
        }
        out.push_str(&row);
        out.push('\n');
    }
    Ok(out)
}
fn comm(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options("comm", args, "123", "D", &[("output-delimiter", 'D')])?;
    if operands.len() != 2 {
        return Err(Fail::usage(format!(
            "comm: two file operands are required\n{}",
            usage_line("comm")
        )));
    }
    let delimiter = value(&opts, 'D').unwrap_or("\t").to_string();
    let a = read_text(c, "comm", &operands[0], input)?;
    let b = read_text(c, "comm", &operands[1], input)?;
    let (left, right) = (lines_of(&a), lines_of(&b));
    let (mut i, mut j) = (0, 0);
    let mut out = String::new();
    let emit = |column: usize, line: &str, out: &mut String| {
        if flag(&opts, char::from(b'0' + column as u8)) {
            return;
        }
        let mut prefix = String::new();
        for k in 1..column {
            if !flag(&opts, char::from(b'0' + k as u8)) {
                prefix.push_str(&delimiter);
            }
        }
        out.push_str(&format!("{prefix}{line}\n"));
    };
    while i < left.len() || j < right.len() {
        match (left.get(i), right.get(j)) {
            (Some(x), Some(y)) if x == y => {
                emit(3, x, &mut out);
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x < y => {
                emit(1, x, &mut out);
                i += 1;
            }
            (Some(_), Some(y)) => {
                emit(2, y, &mut out);
                j += 1;
            }
            (Some(x), None) => {
                emit(1, x, &mut out);
                i += 1;
            }
            (None, Some(y)) => {
                emit(2, y, &mut out);
                j += 1;
            }
            (None, None) => break,
        }
    }
    Ok(out)
}
fn join(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "join",
        args,
        "i",
        "12jtaveo",
        &[
            ("ignore-case", 'i'),
            ("field-separator", 't'),
            ("check-order", 'C'),
            ("nocheck-order", 'N'),
        ],
    )?;
    // `--check-order` really checks; `--nocheck-order` names the default, because this
    // implementation never warns about order on its own.
    let check_order = flag(&opts, 'C') && !flag(&opts, 'N');
    if operands.len() != 2 {
        return Err(Fail::usage(format!(
            "join: two file operands are required\n{}",
            usage_line("join")
        )));
    }
    let field = |letter: char, default: usize| -> Result<usize, Fail> {
        match value(&opts, letter).or_else(|| value(&opts, 'j')) {
            Some(v) => count("join", "field number", v),
            None => Ok(default),
        }
    };
    let f1 = field('1', 1)?;
    let f2 = field('2', 1)?;
    let separator = match value(&opts, 't') {
        Some(raw) => {
            let chars: Vec<char> = unescape(raw).chars().collect();
            match chars.len() {
                1 => Some(chars[0]),
                _ => {
                    return Err(Fail::usage(
                        "join: the field separator must be a single character",
                    ))
                }
            }
        }
        None => None,
    };
    let empty = value(&opts, 'e').unwrap_or("").to_string();
    let show_unpairable: Vec<usize> = opts
        .iter()
        .filter(|(k, _)| *k == 'a')
        .map(|(_, v)| v.parse::<usize>().unwrap_or(0))
        .collect();
    let only_unpairable: Vec<usize> = opts
        .iter()
        .filter(|(k, _)| *k == 'v')
        .map(|(_, v)| v.parse::<usize>().unwrap_or(0))
        .collect();
    let format: Option<Vec<(usize, usize)>> = match value(&opts, 'o') {
        Some(spec) => {
            let mut parts = Vec::new();
            for item in spec.split([',', ' ']).filter(|s| !s.is_empty()) {
                if item == "0" {
                    parts.push((0, 0));
                    continue;
                }
                let (file, field) = item.split_once('.').ok_or_else(|| {
                    Fail::usage(format!("join: invalid field specifier '{item}'"))
                })?;
                parts.push((
                    file.parse().map_err(|_| {
                        Fail::usage(format!("join: invalid file number in '{item}'"))
                    })?,
                    field.parse().map_err(|_| {
                        Fail::usage(format!("join: invalid field number in '{item}'"))
                    })?,
                ));
            }
            Some(parts)
        }
        None => None,
    };
    let split = |line: &str| -> Vec<String> {
        match separator {
            Some(t) => line.split(t).map(str::to_string).collect(),
            None => line.split_whitespace().map(str::to_string).collect(),
        }
    };
    let output_sep = separator.map_or(String::from(" "), |c| c.to_string());
    let a = read_text(c, "join", &operands[0], input)?;
    let b = read_text(c, "join", &operands[1], input)?;
    let rows_a: Vec<Vec<String>> = lines_of(&a).iter().map(|l| split(l)).collect();
    let rows_b: Vec<Vec<String>> = lines_of(&b).iter().map(|l| split(l)).collect();
    if check_order {
        for (rows, file, field) in [(&rows_a, &operands[0], f1), (&rows_b, &operands[1], f2)] {
            for pair in rows.windows(2) {
                let (x, y) = (
                    pair[0].get(field - 1).cloned().unwrap_or_default(),
                    pair[1].get(field - 1).cloned().unwrap_or_default(),
                );
                if x > y {
                    return Err(Fail::new(format!("join: {file} is not sorted"), 1));
                }
            }
        }
    }
    let key = |row: &[String], n: usize| -> String {
        let raw = row.get(n - 1).cloned().unwrap_or_default();
        if flag(&opts, 'i') {
            raw.to_uppercase()
        } else {
            raw
        }
    };
    let mut out = String::new();
    let (mut i, mut j) = (0, 0);
    let mut paired_b = vec![false; rows_b.len()];
    while i < rows_a.len() {
        let ka = key(&rows_a[i], f1);
        // Advance b to the first row whose key is not smaller.
        while j < rows_b.len() && key(&rows_b[j], f2) < ka {
            if (only_unpairable.contains(&2) || show_unpairable.contains(&2)) && !paired_b[j] {
                out.push_str(&rows_b[j].join(&output_sep));
                out.push('\n');
            }
            j += 1;
        }
        let mut matched = false;
        let mut k = j;
        while k < rows_b.len() && key(&rows_b[k], f2) == ka {
            matched = true;
            paired_b[k] = true;
            if only_unpairable.is_empty() {
                let row = match &format {
                    Some(parts) => parts
                        .iter()
                        .map(|(file, field)| match file {
                            0 => rows_a[i].get(f1 - 1).cloned().unwrap_or_default(),
                            1 => rows_a[i].get(field - 1).cloned().unwrap_or(empty.clone()),
                            _ => rows_b[k].get(field - 1).cloned().unwrap_or(empty.clone()),
                        })
                        .collect::<Vec<_>>(),
                    None => {
                        let mut row = vec![rows_a[i].get(f1 - 1).cloned().unwrap_or_default()];
                        row.extend(
                            rows_a[i]
                                .iter()
                                .enumerate()
                                .filter(|(n, _)| *n != f1 - 1)
                                .map(|(_, v)| v.clone()),
                        );
                        row.extend(
                            rows_b[k]
                                .iter()
                                .enumerate()
                                .filter(|(n, _)| *n != f2 - 1)
                                .map(|(_, v)| v.clone()),
                        );
                        row
                    }
                };
                out.push_str(&row.join(&output_sep));
                out.push('\n');
            }
            k += 1;
        }
        if !matched && (show_unpairable.contains(&1) || only_unpairable.contains(&1)) {
            out.push_str(&rows_a[i].join(&output_sep));
            out.push('\n');
        }
        i += 1;
    }
    if show_unpairable.contains(&2) || only_unpairable.contains(&2) {
        for (k, row) in rows_b.iter().enumerate().skip(j) {
            if !paired_b[k] {
                out.push_str(&row.join(&output_sep));
                out.push('\n');
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- small filters

fn nl(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "nl",
        args,
        "",
        "bnwsv",
        &[
            ("body-numbering", 'b'),
            ("number-format", 'n'),
            ("number-width", 'w'),
            ("number-separator", 's'),
            ("starting-line-number", 'v'),
        ],
    )?;
    let style = value(&opts, 'b').unwrap_or("t").to_string();
    let width = match value(&opts, 'w') {
        Some(v) => count("nl", "line-number width", v)?,
        None => 6,
    };
    let separator = value(&opts, 's')
        .map(unescape)
        .unwrap_or_else(|| "\t".into());
    let format = value(&opts, 'n').unwrap_or("rn").to_string();
    let mut n: i64 = match value(&opts, 'v') {
        Some(v) => v
            .parse()
            .map_err(|_| Fail::usage(format!("nl: invalid starting line number: '{v}'")))?,
        None => 1,
    };
    let matcher = match style.as_str() {
        "a" | "t" | "n" => None,
        other => match other.strip_prefix('p') {
            Some(re) => Some(
                regex::Regex::new(&crate::sed::translate(re, false)?)
                    .map_err(|e| Fail::usage(format!("nl: invalid regular expression: {e}")))?,
            ),
            None => {
                return Err(Fail::usage(format!(
                    "nl: invalid body numbering style: '{other}'"
                )))
            }
        },
    };
    let mut out = String::new();
    for (_, text) in sources(c, "nl", &operands, input)? {
        for line in lines_of(&text) {
            let numbered = match (style.as_str(), &matcher) {
                ("a", _) => true,
                ("n", _) => false,
                ("t", _) => !line.is_empty(),
                (_, Some(re)) => re.is_match(line),
                _ => true,
            };
            if numbered {
                let body = match format.as_str() {
                    "ln" => format!("{n:<width$}"),
                    "rz" => format!("{n:0width$}"),
                    "rn" => format!("{n:>width$}"),
                    other => {
                        return Err(Fail::usage(format!("nl: invalid number format: '{other}'")))
                    }
                };
                out.push_str(&format!("{body}{separator}{line}\n"));
                n += 1;
            } else {
                out.push_str(&format!("{}{line}\n", " ".repeat(width + separator.len())));
            }
        }
    }
    Ok(out)
}
fn rev(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (_, operands) = options("rev", args, "", "", &[])?;
    let mut out = String::new();
    for (_, text) in sources(c, "rev", &operands, input)? {
        for line in lines_of(&text) {
            out.push_str(&line.chars().rev().collect::<String>());
            out.push('\n');
        }
    }
    Ok(out)
}
fn fold(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let rewritten: Vec<String> = args
        .iter()
        .map(|a| match a.strip_prefix('-') {
            Some(d) if !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()) => {
                format!("-w{d}")
            }
            _ => a.clone(),
        })
        .collect();
    let (opts, operands) = options(
        "fold",
        &rewritten,
        "sb",
        "w",
        &[("spaces", 's'), ("bytes", 'b'), ("width", 'w')],
    )?;
    let width = match value(&opts, 'w') {
        Some(v) => count("fold", "width", v)?,
        None => 80,
    };
    if width == 0 {
        return Err(Fail::usage("fold: invalid number of columns: '0'"));
    }
    let mut out = String::new();
    for (_, text) in sources(c, "fold", &operands, input)? {
        for line in lines_of(&text) {
            let units: Vec<String> = if flag(&opts, 'b') {
                line.bytes().map(|b| (b as char).to_string()).collect()
            } else {
                line.chars().map(|c| c.to_string()).collect()
            };
            if units.is_empty() {
                out.push('\n');
                continue;
            }
            let mut start = 0;
            while start < units.len() {
                let mut end = (start + width).min(units.len());
                if flag(&opts, 's') && end < units.len() {
                    if let Some(space) = (start..end).rev().find(|k| units[*k] == " ") {
                        if space > start {
                            end = space + 1;
                        }
                    }
                }
                out.push_str(&units[start..end].concat());
                out.push('\n');
                start = end;
            }
        }
    }
    Ok(out)
}
fn expand_tabs(c: &Computer, cmd: &str, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        cmd,
        args,
        "ai",
        "t",
        &[("tabs", 't'), ("all", 'a'), ("initial", 'i')],
    )?;
    // The two commands do not share a flag set: `-i` is expand's, `-a` unexpand's.
    if cmd == "expand" && flag(&opts, 'a') {
        return Err(invalid_option("expand", 'a'));
    }
    if cmd == "unexpand" && flag(&opts, 'i') {
        return Err(invalid_option("unexpand", 'i'));
    }
    let stops: Vec<usize> = match value(&opts, 't') {
        Some(v) => {
            let mut list = Vec::new();
            for part in v.split([',', ' ']).filter(|s| !s.is_empty()) {
                list.push(count(cmd, "tab size", part)?);
            }
            if list.contains(&0) {
                return Err(Fail::usage(format!("{cmd}: tab size cannot be 0")));
            }
            list
        }
        None => vec![8],
    };
    let next_stop = |column: usize| -> usize {
        if stops.len() == 1 {
            return column + stops[0] - column % stops[0];
        }
        for s in &stops {
            if *s > column {
                return *s;
            }
        }
        column + 1
    };
    let mut out = String::new();
    for (_, text) in sources(c, cmd, &operands, input)? {
        for line in lines_of(&text) {
            if cmd == "expand" {
                let mut column = 0;
                let mut leading = true;
                for ch in line.chars() {
                    if ch == '\t' && (!flag(&opts, 'i') || leading) {
                        let target = next_stop(column);
                        out.push_str(&" ".repeat(target - column));
                        column = target;
                    } else {
                        if ch != ' ' {
                            leading = false;
                        }
                        out.push(ch);
                        column += 1;
                    }
                }
            } else {
                out.push_str(&unexpand_line(line, &stops, flag(&opts, 'a')));
            }
            out.push('\n');
        }
    }
    Ok(out)
}
fn unexpand_line(line: &str, stops: &[usize], all: bool) -> String {
    let width = stops[0];
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut column = 0;
    let mut i = 0;
    let mut leading = true;
    while i < chars.len() {
        if chars[i] == ' ' && (all || leading) {
            let start = i;
            let start_column = column;
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
                column += 1;
            }
            let mut at = start_column;
            let mut emitted = String::new();
            while at + (width - at % width) <= column && at + (width - at % width) > at {
                let target = at + (width - at % width);
                emitted.push('\t');
                at = target;
            }
            emitted.push_str(&" ".repeat(column - at));
            // Only shorter output is worth the substitution.
            if emitted.len() <= i - start {
                out.push_str(&emitted);
            } else {
                out.push_str(&" ".repeat(i - start));
            }
            continue;
        }
        if chars[i] != ' ' {
            leading = false;
        }
        out.push(chars[i]);
        column += 1;
        i += 1;
    }
    out
}
fn tee(c: &mut Computer, args: &[String], input: &str, t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "tee",
        args,
        "ai",
        "",
        &[("append", 'a'), ("ignore-interrupts", 'i')],
    )?;
    for name in &operands {
        if name == "/dev/null" {
            continue;
        }
        let path = c.resolve(name);
        let r = if flag(&opts, 'a') {
            c.vfs.append(&path, input.as_bytes(), &c.user, t)
        } else {
            c.vfs.write_as(&path, input.as_bytes(), &c.user, t)
        };
        r.map_err(|e| Fail::io("tee", name, &e))?;
    }
    Ok(input.to_string())
}
fn seq(args: &[String]) -> Result<String, Fail> {
    let (opts, operands) = options(
        "seq",
        args,
        "w",
        "sf",
        &[("separator", 's'), ("format", 'f'), ("equal-width", 'w')],
    )?;
    let numbers: Vec<f64> = operands
        .iter()
        .map(|o| {
            o.parse::<f64>()
                .map_err(|_| Fail::usage(format!("seq: invalid floating point argument: '{o}'")))
        })
        .collect::<Result<_, _>>()?;
    let (first, step, last) = match numbers.len() {
        1 => (1.0, 1.0, numbers[0]),
        2 => (numbers[0], 1.0, numbers[1]),
        3 => (numbers[0], numbers[1], numbers[2]),
        _ => {
            return Err(Fail::usage(format!(
                "seq: missing operand\n{}",
                usage_line("seq")
            )))
        }
    };
    if step == 0.0 {
        return Err(Fail::usage("seq: invalid Zero increment value: '0'"));
    }
    let separator = value(&opts, 's')
        .map(unescape)
        .unwrap_or_else(|| "\n".into());
    let format = value(&opts, 'f').map(str::to_string);
    let decimals = |v: f64| -> usize {
        let s = format!("{v}");
        s.split_once('.').map_or(0, |(_, d)| d.len())
    };
    let places = decimals(first).max(decimals(step)).max(decimals(last));
    let mut values = Vec::new();
    let mut v = first;
    let mut guard = 0;
    while (step > 0.0 && v <= last + 1e-9) || (step < 0.0 && v >= last - 1e-9) {
        values.push(v);
        v += step;
        guard += 1;
        if guard > 1_000_000 {
            return Err(Fail::usage("seq: refusing to generate over 1000000 values"));
        }
    }
    let rendered: Vec<String> = values
        .iter()
        .map(|v| match &format {
            Some(f) => crate::awk::format_spec(f, &[crate::awk::scalar_num(*v)])
                .unwrap_or_else(|_| format!("{v}")),
            None => format!("{v:.places$}"),
        })
        .collect();
    let rendered = if flag(&opts, 'w') && format.is_none() {
        let width = rendered.iter().map(String::len).max().unwrap_or(0);
        rendered
            .into_iter()
            .map(|s| {
                let negative = s.starts_with('-');
                let body = if negative { &s[1..] } else { &s[..] };
                let pad = width - s.len();
                if negative {
                    format!("-{}{body}", "0".repeat(pad))
                } else {
                    format!("{}{body}", "0".repeat(pad))
                }
            })
            .collect()
    } else {
        rendered
    };
    if rendered.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("{}\n", rendered.join(&separator)))
}
/// `yes` in a world whose pipelines are not lazy: it writes a bounded number of
/// copies and stops, instead of never returning. The bound is published.
const YES_LIMIT: usize = 10_000;
fn yes(args: &[String]) -> Result<String, Fail> {
    // GNU's getopt runs before the operand, so a long option is refused, not echoed.
    if let Some(bad) = args.first().and_then(|a| a.strip_prefix("--")) {
        if !bad.is_empty() {
            return Err(unrecognized_option("yes", bad));
        }
    }
    let text = if args.is_empty() {
        String::from("y")
    } else {
        args.join(" ")
    };
    let mut out = String::with_capacity((text.len() + 1) * YES_LIMIT);
    for _ in 0..YES_LIMIT {
        out.push_str(&text);
        out.push('\n');
    }
    Ok(out)
}
fn shuf(c: &Computer, args: &[String], input: &str, t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "shuf",
        args,
        "erz",
        "ni",
        &[
            ("echo", 'e'),
            ("repeat", 'r'),
            ("head-count", 'n'),
            ("input-range", 'i'),
            ("zero-terminated", 'z'),
        ],
    )?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(
            "shuf: -z (NUL-terminated lines) is not modelled by this world",
        ));
    }
    let mut items: Vec<String> = if flag(&opts, 'e') {
        operands.clone()
    } else if let Some(range) = value(&opts, 'i') {
        let (lo, hi) = range
            .split_once('-')
            .ok_or_else(|| Fail::usage(format!("shuf: invalid input range '{range}'")))?;
        let lo: i64 = lo
            .parse()
            .map_err(|_| Fail::usage(format!("shuf: invalid input range '{range}'")))?;
        let hi: i64 = hi
            .parse()
            .map_err(|_| Fail::usage(format!("shuf: invalid input range '{range}'")))?;
        (lo..=hi).map(|n| n.to_string()).collect()
    } else {
        let text = read_text(
            c,
            "shuf",
            operands.first().map_or("-", String::as_str),
            input,
        )?;
        lines_of(&text).iter().map(|s| (*s).to_string()).collect()
    };
    // Seeded from the tick and the machine, never from a host RNG.
    let mut state = t.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(
        c.id.bytes()
            .fold(1u64, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b))),
    );
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let take = match value(&opts, 'n') {
        Some(v) => Some(count("shuf", "lines", v)?),
        None => None,
    };
    if flag(&opts, 'r') {
        let want = take.unwrap_or(YES_LIMIT);
        if items.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::new();
        for _ in 0..want {
            let i = (next() % items.len() as u64) as usize;
            out.push_str(&items[i]);
            out.push('\n');
        }
        return Ok(out);
    }
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
    if let Some(n) = take {
        items.truncate(n);
    }
    Ok(joined(&items))
}

// ---------------------------------------------------------------- paths

fn basename(args: &[String]) -> Result<String, Fail> {
    let (opts, operands) = options(
        "basename",
        args,
        "az",
        "s",
        &[("multiple", 'a'), ("suffix", 's'), ("zero", 'z')],
    )?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(
            "basename: -z (NUL output) is not modelled by this world",
        ));
    }
    if operands.is_empty() {
        return Err(Fail::usage(format!(
            "basename: missing operand\n{}",
            usage_line("basename")
        )));
    }
    let multiple = flag(&opts, 'a') || value(&opts, 's').is_some();
    let (names, suffix) = if multiple {
        (operands.as_slice(), value(&opts, 's').map(str::to_string))
    } else {
        (&operands[..1], operands.get(1).cloned())
    };
    if !multiple && operands.len() > 2 {
        return Err(Fail::usage(format!(
            "basename: extra operand '{}'\n{}",
            operands[2],
            usage_line("basename")
        )));
    }
    let mut out = String::new();
    for name in names {
        let trimmed = name.trim_end_matches('/');
        let base = if trimmed.is_empty() {
            if name.is_empty() {
                ""
            } else {
                "/"
            }
        } else {
            trimmed.rsplit('/').next().unwrap_or(trimmed)
        };
        let base = match &suffix {
            Some(s) if base != s && base.ends_with(s.as_str()) => &base[..base.len() - s.len()],
            _ => base,
        };
        out.push_str(base);
        out.push('\n');
    }
    Ok(out)
}
fn dirname(args: &[String]) -> Result<String, Fail> {
    let (opts, operands) = options("dirname", args, "z", "", &[("zero", 'z')])?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(
            "dirname: -z (NUL output) is not modelled by this world",
        ));
    }
    if operands.is_empty() {
        return Err(Fail::usage(format!(
            "dirname: missing operand\n{}",
            usage_line("dirname")
        )));
    }
    let mut out = String::new();
    for name in &operands {
        let trimmed = name.trim_end_matches('/');
        let head = match trimmed.rfind('/') {
            None => ".",
            Some(0) => "/",
            Some(k) => &trimmed[..k],
        };
        let head = if trimmed.is_empty() && name.starts_with('/') {
            "/"
        } else {
            head
        };
        out.push_str(head);
        out.push('\n');
    }
    Ok(out)
}
fn realpath(c: &Computer, cmd: &str, args: &[String]) -> Result<String, Fail> {
    let (opts, operands) = options(
        cmd,
        args,
        "emsfnqz",
        "",
        &[
            ("canonicalize-missing", 'm'),
            ("canonicalize-existing", 'e'),
            ("no-symlinks", 's'),
            ("canonicalize", 'f'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("zero", 'z'),
        ],
    )?;
    if flag(&opts, 'z') {
        return Err(Fail::usage(format!(
            "{cmd}: -z (NUL output) is not modelled by this world"
        )));
    }
    if operands.is_empty() {
        return Err(Fail::usage(format!(
            "{cmd}: missing operand\n{}",
            usage_line(cmd)
        )));
    }
    // `readlink` prints only the link target unless -f/-e/-m asks for resolution.
    let resolve_links =
        cmd == "realpath" || flag(&opts, 'f') || flag(&opts, 'e') || flag(&opts, 'm');
    let mut out = String::new();
    for name in &operands {
        let path = c.resolve(name);
        if cmd == "readlink" && !resolve_links {
            match c.vfs.read_link(&path) {
                Ok(target) => {
                    out.push_str(&target);
                    out.push('\n');
                }
                Err(e) => {
                    if flag(&opts, 'q') {
                        continue;
                    }
                    return Err(Fail::io(cmd, name, &e));
                }
            }
            continue;
        }
        let canonical = if flag(&opts, 's') {
            path.clone()
        } else {
            follow(c, &path)
        };
        let exists = c.vfs.exists(&canonical);
        if !exists && (flag(&opts, 'e') || (cmd == "readlink" && !flag(&opts, 'm'))) {
            if flag(&opts, 'q') {
                continue;
            }
            return Err(Fail::op(cmd, name, "No such file or directory"));
        }
        out.push_str(&canonical);
        out.push('\n');
    }
    Ok(out)
}
/// Resolves every symlink on the path, bounded so a loop cannot hang the world.
fn follow(c: &Computer, path: &str) -> String {
    let mut current = String::from("/");
    for component in path.split('/').filter(|s| !s.is_empty()) {
        let joined = if current == "/" {
            format!("/{component}")
        } else {
            format!("{current}/{component}")
        };
        current = joined;
        for _ in 0..16 {
            match c.vfs.read_link(&current) {
                Ok(target) => {
                    current = if target.starts_with('/') {
                        crate::normalize_path("/", &target)
                    } else {
                        let parent = current.rsplit_once('/').map_or("/", |(a, _)| a);
                        let parent = if parent.is_empty() { "/" } else { parent };
                        crate::normalize_path(parent, &target)
                    };
                }
                Err(_) => break,
            }
        }
    }
    current
}

// ---------------------------------------------------------------- printf

/// C escapes in a `printf` format: `\n`, `\t`, `\0NNN`, `\xHH` and the rest.
pub(crate) fn printf_escapes(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        match b[i] {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            'e' => out.push('\u{1b}'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            'c' => return out,
            // `\0NNN` is POSIX; `\NNN` is what every shell also accepts.
            c if c.is_digit(8) => {
                if c == '0' {
                    i += 1;
                }
                let mut code = 0u32;
                let mut used = 0;
                while used < 3 && b.get(i).is_some_and(|c| c.is_digit(8)) {
                    code = code * 8 + b[i].to_digit(8).unwrap();
                    i += 1;
                    used += 1;
                }
                out.push(char::from_u32(code).unwrap_or('\0'));
                continue;
            }
            'x' => {
                i += 1;
                let mut code = 0u32;
                let mut used = 0;
                while used < 2 && b.get(i).is_some_and(char::is_ascii_hexdigit) {
                    code = code * 16 + b[i].to_digit(16).unwrap();
                    i += 1;
                    used += 1;
                }
                out.push(char::from_u32(code).unwrap_or('\0'));
                continue;
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
        i += 1;
    }
    out
}
/// The shell's own `printf`: the format is reused until the operands run out, which
/// is the behaviour scripts rely on for `printf '%s\n' a b c`.
pub(crate) fn shell_printf(args: &[String]) -> Result<String, Fail> {
    let Some(raw) = args.first() else {
        return Err(Fail::usage(format!(
            "printf: missing operand\n{}",
            usage_line("printf")
        )));
    };
    // Unlike awk, whose escapes are resolved when the string literal is lexed, the
    // shell's printf receives its format as an operand and must expand it itself.
    let format = &printf_escapes(raw);
    let rest = &args[1..];
    let conversions = format
        .match_indices('%')
        .filter(|(i, _)| {
            format[i + 1..].chars().next().is_some_and(|c| c != '%')
                && format[..*i].chars().rev().take_while(|c| *c == '%').count() % 2 == 0
        })
        .count();
    let values: Vec<crate::awk::Value> = rest.iter().map(|s| crate::awk::scalar(s)).collect();
    let mut out = String::new();
    if conversions == 0 || values.is_empty() {
        out.push_str(&crate::awk::format_spec(format, &values)?);
        return Ok(out);
    }
    let mut at = 0;
    while at < values.len() {
        let end = (at + conversions).min(values.len());
        out.push_str(&crate::awk::format_spec(format, &values[at..end])?);
        at = end.max(at + 1);
    }
    Ok(out)
}

// ---------------------------------------------------------------- split / strings

fn split_file(c: &mut Computer, args: &[String], input: &str, t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "split",
        args,
        "d",
        "lba",
        &[
            ("lines", 'l'),
            ("bytes", 'b'),
            ("suffix-length", 'a'),
            ("numeric-suffixes", 'd'),
        ],
    )?;
    let suffix_length = match value(&opts, 'a') {
        Some(v) => count("split", "suffix length", v)?,
        None => 2,
    };
    let text = read_text(
        c,
        "split",
        operands.first().map_or("-", String::as_str),
        input,
    )?;
    let prefix = operands.get(1).cloned().unwrap_or_else(|| "x".into());
    let chunks: Vec<String> = match value(&opts, 'b') {
        Some(v) => {
            let n = size("split", v)?.max(1);
            text.as_bytes()
                .chunks(n)
                .map(|c| String::from_utf8_lossy(c).into_owned())
                .collect()
        }
        None => {
            let n = match value(&opts, 'l') {
                Some(v) => count("split", "lines", v)?.max(1),
                None => 1000,
            };
            lines_of(&text)
                .chunks(n)
                .map(|c| joined(c.iter().copied()))
                .collect()
        }
    };
    let alphabet: Vec<char> = if flag(&opts, 'd') {
        ('0'..='9').collect()
    } else {
        ('a'..='z').collect()
    };
    let limit = alphabet.len().pow(suffix_length as u32);
    if chunks.len() > limit {
        return Err(Fail::new(
            format!("split: output file suffixes exhausted at length {suffix_length}"),
            1,
        ));
    }
    for (i, chunk) in chunks.iter().enumerate() {
        let mut suffix = String::new();
        let mut n = i;
        for _ in 0..suffix_length {
            suffix.insert(0, alphabet[n % alphabet.len()]);
            n /= alphabet.len();
        }
        let path = c.resolve(&format!("{prefix}{suffix}"));
        c.vfs
            .write_as(&path, chunk.as_bytes(), &c.user, t)
            .map_err(|e| Fail::io("split", format!("{prefix}{suffix}"), &e))?;
    }
    Ok(String::new())
}
fn strings(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options("strings", args, "a", "n", &[("bytes", 'n'), ("all", 'a')])?;
    let minimum = match value(&opts, 'n') {
        Some(v) => count("strings", "bytes", v)?.max(1),
        None => 4,
    };
    let mut out = String::new();
    let names: Vec<String> = if operands.is_empty() {
        vec![String::from("-")]
    } else {
        operands.clone()
    };
    for name in &names {
        let bytes = read_bytes(c, "strings", name, input)?;
        let mut run = String::new();
        for b in bytes.iter().chain([&0u8]) {
            if (0x20..0x7f).contains(b) || *b == b'\t' {
                run.push(char::from(*b));
            } else {
                if run.chars().count() >= minimum {
                    out.push_str(&run);
                    out.push('\n');
                }
                run.clear();
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- xargs

/// Splits xargs input the way xargs does: blanks separate, quotes group, `\` escapes.
fn xargs_items(text: &str, delimiter: Option<char>, nul: bool) -> Result<Vec<String>, Fail> {
    if nul {
        return Ok(text
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect());
    }
    if let Some(d) = delimiter {
        let body = text.strip_suffix('\n').unwrap_or(text);
        let mut items: Vec<String> = body.split(d).map(str::to_string).collect();
        // Input that ends with the delimiter does not produce a final empty argument.
        if items.last().is_some_and(String::is_empty) {
            items.pop();
        }
        return Ok(items);
    }
    let mut items = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' | '\t' | '\n' => {
                if started {
                    items.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' | '"' => {
                started = true;
                let quote = ch;
                loop {
                    match chars.next() {
                        None => {
                            return Err(Fail::new(
                                format!(
                                    "xargs: unmatched {} quote",
                                    if quote == '\'' { "single" } else { "double" }
                                ),
                                1,
                            ))
                        }
                        Some(c) if c == quote => break,
                        Some(c) => current.push(c),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(c) = chars.next() {
                    current.push(c);
                }
            }
            other => {
                started = true;
                current.push(other);
            }
        }
    }
    if started {
        items.push(current);
    }
    Ok(items)
}
fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/=:,+@".contains(c))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}
fn xargs(
    c: &mut Computer,
    args: &[String],
    input: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, Fail> {
    let mut nul = false;
    let mut delimiter: Option<char> = None;
    let mut max_args: Option<usize> = None;
    let mut replace: Option<String> = None;
    let mut run_if_empty = true;
    let mut trace = false;
    let mut command: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].clone();
        if a == "--" {
            i += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        let mut take = |name: &str, inline: String| -> Result<String, Fail> {
            if inline.is_empty() {
                i += 1;
                args.get(i)
                    .cloned()
                    .ok_or_else(|| missing_argument("xargs", name))
            } else {
                Ok(inline)
            }
        };
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((k, v)) => (k, v.to_string()),
                None => (long, String::new()),
            };
            match name {
                "null" => nul = true,
                "no-run-if-empty" => run_if_empty = false,
                "verbose" => trace = true,
                "delimiter" => {
                    let v = take("delimiter", inline)?;
                    delimiter = unescape(&v).chars().next();
                }
                "max-args" => {
                    let v = take("max-args", inline)?;
                    max_args = Some(count("xargs", "arguments", &v)?);
                }
                "replace" => replace = Some(take("replace", inline)?),
                "max-procs" => {
                    take("max-procs", inline)?;
                }
                _ => return Err(unrecognized_option("xargs", name)),
            }
            i += 1;
            continue;
        }
        let letters: Vec<char> = a[1..].chars().collect();
        let mut j = 0;
        while j < letters.len() {
            let ch = letters[j];
            let inline: String = letters[j + 1..].iter().collect();
            match ch {
                '0' => {
                    nul = true;
                    j += 1;
                    continue;
                }
                'r' => {
                    run_if_empty = false;
                    j += 1;
                    continue;
                }
                't' => {
                    trace = true;
                    j += 1;
                    continue;
                }
                'd' => {
                    let v = take("d", inline)?;
                    delimiter = unescape(&v).chars().next();
                }
                'n' => {
                    let v = take("n", inline)?;
                    max_args = Some(count("xargs", "arguments", &v)?);
                }
                'I' => replace = Some(take("I", inline)?),
                // Parallelism is accepted and then ignored *by contract*: commands run
                // one after another so a replay is identical. Documented in shell.md.
                'P' => {
                    take("P", inline)?;
                }
                other => return Err(invalid_option("xargs", other)),
            }
            j = letters.len();
        }
        i += 1;
    }
    command.extend(args[i.min(args.len())..].iter().cloned());
    if command.is_empty() {
        command.push(String::from("echo"));
    }
    let items = xargs_items(input, delimiter, nul)?;
    if items.is_empty() && !run_if_empty {
        return Ok(String::new());
    }
    let batches: Vec<Vec<String>> = match (&replace, max_args) {
        (Some(_), _) => items.iter().map(|s| vec![s.clone()]).collect(),
        (None, Some(n)) if n > 0 => items.chunks(n).map(<[String]>::to_vec).collect(),
        _ => vec![items.clone()],
    };
    let batches = if batches.is_empty() {
        vec![Vec::new()]
    } else {
        batches
    };
    let mut out = String::new();
    let mut err = String::new();
    let mut status = 0;
    for batch in batches {
        let line = match &replace {
            Some(token) => command
                .iter()
                .map(|word| {
                    if word.contains(token.as_str()) {
                        quote(
                            &word.replace(token.as_str(), batch.first().map_or("", String::as_str)),
                        )
                    } else {
                        quote(word)
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
            None => command
                .iter()
                .map(|w| quote(w))
                .chain(batch.iter().map(|a| quote(a)))
                .collect::<Vec<_>>()
                .join(" "),
        };
        if trace {
            err.push_str(&line);
            err.push('\n');
        }
        let r = crate::shell::run_piped(c, &line, "", t, host, depth);
        out.push_str(&r.stdout);
        err.push_str(&r.stderr);
        // xargs' own status vocabulary: 123 if any command failed, 127/126 passed on.
        status = match r.exit_code {
            0 => status,
            127 => 127,
            126 => 126,
            255 => 124,
            _ if status == 0 => 123,
            _ => status,
        };
        if matches!(r.exit_code, 126 | 127 | 255) {
            break;
        }
    }
    if status != 0 || !err.is_empty() {
        if err.ends_with('\n') {
            err.pop();
        }
        return Err(Fail::new(err, status).raw().with_output(out));
    }
    Ok(out)
}

// ---------------------------------------------------------------- dispatch

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    c: &mut Computer,
    cmd: &str,
    args: &[String],
    input: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Option<Result<String, Fail>> {
    Some(match cmd {
        "cut" => cut(c, args, input),
        "sort" => sort(c, args, input, t),
        "uniq" => uniq(c, args, input),
        "head" | "tail" => head_tail(c, cmd, args, input),
        "wc" => wc(c, args, input),
        "tr" => tr(args, input),
        "paste" => paste(c, args, input),
        "comm" => comm(c, args, input),
        "join" => join(c, args, input),
        "nl" => nl(c, args, input),
        "rev" => rev(c, args, input),
        "fold" => fold(c, args, input),
        "expand" | "unexpand" => expand_tabs(c, cmd, args, input),
        "tee" => tee(c, args, input, t),
        "seq" => seq(args),
        "yes" => yes(args),
        "shuf" => shuf(c, args, input, t),
        "basename" => basename(args),
        "dirname" => dirname(args),
        "realpath" | "readlink" => realpath(c, cmd, args),
        "printf" => shell_printf(args),
        "split" => split_file(c, args, input, t),
        "strings" => strings(c, args, input),
        "xargs" => xargs(c, args, input, t, host, depth),
        _ => return None,
    })
}
