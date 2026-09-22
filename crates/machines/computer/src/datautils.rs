//! Comparison, digest, encoding and dump utilities: `diff`, `cmp`, `base64`,
//! `md5sum`/`sha1sum`/`sha256sum`, `xxd`, `od`, `hexdump` and `file`.
//!
//! `file`'s magic table covers exactly the formats this world actually produces, and
//! says so; it never guesses at a format nothing here can create.
use crate::shell::{flag, options, read_bytes, read_text, usage_line, value, Fail};
use crate::Computer;

fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}

// ---------------------------------------------------------------- diff

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op {
    Keep,
    Delete,
    Insert,
}
/// Myers' O(ND) diff. The world's files are small, and this keeps the edit script
/// minimal rather than merely correct, so `diff -u` output matches GNU's shape.
fn script(a: &[String], b: &[String]) -> Vec<(Op, usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let max = n + m;
    let offset = max as isize;
    let mut v = vec![0isize; 2 * max + 1];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let index = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[index - 1] < v[index + 1]) {
                v[index + 1]
            } else {
                v[index - 1] + 1
            };
            let mut y = x - k;
            while (x as usize) < n && (y as usize) < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x as usize >= n && y as usize >= m {
                found = Some(d);
                break;
            }
            k += 2;
        }
        if found.is_some() {
            break;
        }
    }
    let Some(d_final) = found else {
        return Vec::new();
    };
    // Walk the saved frontiers backwards to recover the script.
    let mut ops: Vec<(Op, usize, usize)> = Vec::new();
    let (mut x, mut y) = (n as isize, m as isize);
    for d in (0..=d_final).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let index = (k + offset) as usize;
        let previous_k = if k == -d || (k != d && v[index - 1] < v[index + 1]) {
            k + 1
        } else {
            k - 1
        };
        let previous_x = v[(previous_k + offset) as usize];
        let previous_y = previous_x - previous_k;
        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push((Op::Keep, x as usize, y as usize));
        }
        if d > 0 {
            if x == previous_x {
                y -= 1;
                ops.push((Op::Insert, x as usize, y as usize));
            } else {
                x -= 1;
                ops.push((Op::Delete, x as usize, y as usize));
            }
        }
    }
    ops.reverse();
    ops
}
struct Hunk {
    a_start: usize,
    a_len: usize,
    b_start: usize,
    b_len: usize,
    rows: Vec<(Op, String)>,
}
fn hunks(ops: &[(Op, usize, usize)], a: &[String], b: &[String], context: usize) -> Vec<Hunk> {
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (op, _, _))| *op != Op::Keep)
        .map(|(i, _)| i)
        .collect();
    let mut out: Vec<Hunk> = Vec::new();
    let mut i = 0;
    while i < changed.len() {
        let start = changed[i].saturating_sub(context);
        let mut j = i;
        while j + 1 < changed.len() && changed[j + 1] <= changed[j] + 2 * context + 1 {
            j += 1;
        }
        let end = (changed[j] + context + 1).min(ops.len());
        let mut rows = Vec::new();
        let (mut a_start, mut b_start) = (usize::MAX, usize::MAX);
        let (mut a_len, mut b_len) = (0usize, 0usize);
        for (op, ax, bx) in &ops[start..end] {
            match op {
                Op::Keep => {
                    a_start = a_start.min(*ax);
                    b_start = b_start.min(*bx);
                    a_len += 1;
                    b_len += 1;
                    rows.push((Op::Keep, a[*ax].clone()));
                }
                Op::Delete => {
                    a_start = a_start.min(*ax);
                    b_start = b_start.min(*bx);
                    a_len += 1;
                    rows.push((Op::Delete, a[*ax].clone()));
                }
                Op::Insert => {
                    a_start = a_start.min(*ax);
                    b_start = b_start.min(*bx);
                    b_len += 1;
                    rows.push((Op::Insert, b[*bx].clone()));
                }
            }
        }
        out.push(Hunk {
            a_start: if a_start == usize::MAX { 0 } else { a_start },
            a_len,
            b_start: if b_start == usize::MAX { 0 } else { b_start },
            b_len,
            rows,
        });
        i = j + 1;
    }
    out
}
struct DiffOpts {
    unified: Option<usize>,
    context: Option<usize>,
    brief: bool,
    report_identical: bool,
    ignore_case: bool,
    ignore_space_change: bool,
    ignore_all_space: bool,
    ignore_blank_lines: bool,
    recursive: bool,
    new_file: bool,
}
fn normalise(line: &str, o: &DiffOpts) -> String {
    let mut s = line.to_string();
    if o.ignore_all_space {
        s = s.chars().filter(|c| !c.is_whitespace()).collect();
    } else if o.ignore_space_change {
        s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    if o.ignore_case {
        s = s.to_uppercase();
    }
    s
}
fn diff_text(
    label_a: &str,
    label_b: &str,
    text_a: &str,
    text_b: &str,
    o: &DiffOpts,
) -> (String, bool) {
    let raw_a: Vec<String> = lines_of(text_a).iter().map(|s| (*s).to_string()).collect();
    let raw_b: Vec<String> = lines_of(text_b).iter().map(|s| (*s).to_string()).collect();
    let key_a: Vec<String> = raw_a
        .iter()
        .filter(|l| !(o.ignore_blank_lines && l.trim().is_empty()))
        .map(|l| normalise(l, o))
        .collect();
    let key_b: Vec<String> = raw_b
        .iter()
        .filter(|l| !(o.ignore_blank_lines && l.trim().is_empty()))
        .map(|l| normalise(l, o))
        .collect();
    if key_a == key_b {
        let text = if o.report_identical {
            format!("Files {label_a} and {label_b} are identical\n")
        } else {
            String::new()
        };
        return (text, false);
    }
    if o.brief {
        return (format!("Files {label_a} and {label_b} differ\n"), true);
    }
    let ops = script(&raw_a, &raw_b);
    let context = o.unified.or(o.context).unwrap_or(3);
    let groups = hunks(&ops, &raw_a, &raw_b, context);
    let mut out = String::new();
    if o.context.is_some() {
        out.push_str(&format!("*** {label_a}\n--- {label_b}\n"));
        for h in &groups {
            out.push_str("***************\n");
            out.push_str(&format!(
                "*** {},{} ****\n",
                h.a_start + 1,
                h.a_start + h.a_len
            ));
            if h.rows.iter().any(|(op, _)| *op == Op::Delete) {
                for (op, line) in &h.rows {
                    match op {
                        Op::Keep => out.push_str(&format!("  {line}\n")),
                        Op::Delete => out.push_str(&format!("- {line}\n")),
                        Op::Insert => {}
                    }
                }
            }
            out.push_str(&format!(
                "--- {},{} ----\n",
                h.b_start + 1,
                h.b_start + h.b_len
            ));
            if h.rows.iter().any(|(op, _)| *op == Op::Insert) {
                for (op, line) in &h.rows {
                    match op {
                        Op::Keep => out.push_str(&format!("  {line}\n")),
                        Op::Insert => out.push_str(&format!("+ {line}\n")),
                        Op::Delete => {}
                    }
                }
            }
        }
        return (out, true);
    }
    if o.unified.is_some() {
        out.push_str(&format!("--- {label_a}\n+++ {label_b}\n"));
        // GNU writes `N` for a one-line span and `N,0` for an empty one.
        let span = |start: usize, len: usize| match len {
            0 => format!("{start},0"),
            1 => format!("{}", start + 1),
            _ => format!("{},{len}", start + 1),
        };
        for h in &groups {
            out.push_str(&format!(
                "@@ -{} +{} @@\n",
                span(h.a_start, h.a_len),
                span(h.b_start, h.b_len)
            ));
            for (op, line) in &h.rows {
                out.push_str(&match op {
                    Op::Keep => format!(" {line}\n"),
                    Op::Delete => format!("-{line}\n"),
                    Op::Insert => format!("+{line}\n"),
                });
            }
        }
        return (out, true);
    }
    // The default `ed`-flavoured listing.
    for h in &groups {
        let deletes: Vec<&String> = h
            .rows
            .iter()
            .filter(|(op, _)| *op == Op::Delete)
            .map(|(_, l)| l)
            .collect();
        let inserts: Vec<&String> = h
            .rows
            .iter()
            .filter(|(op, _)| *op == Op::Insert)
            .map(|(_, l)| l)
            .collect();
        let leading = h.rows.iter().take_while(|(op, _)| *op == Op::Keep).count();
        let a_from = h.a_start + leading + 1;
        let b_from = h.b_start + leading + 1;
        let range = |from: usize, len: usize| {
            if len <= 1 {
                format!("{}", from.max(if len == 0 { from - 1 } else { from }))
            } else {
                format!("{from},{}", from + len - 1)
            }
        };
        let verb = match (deletes.is_empty(), inserts.is_empty()) {
            (false, false) => 'c',
            (false, true) => 'd',
            _ => 'a',
        };
        out.push_str(&format!(
            "{}{verb}{}\n",
            range(a_from, deletes.len()),
            range(b_from, inserts.len())
        ));
        for line in &deletes {
            out.push_str(&format!("< {line}\n"));
        }
        if verb == 'c' {
            out.push_str("---\n");
        }
        for line in &inserts {
            out.push_str(&format!("> {line}\n"));
        }
    }
    (out, true)
}
fn diff(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let rewritten: Vec<String> = args
        .iter()
        .map(|a| {
            // `-U3`/`-u3` carry their context inline.
            match a.strip_prefix("-U").or_else(|| a.strip_prefix("-u")) {
                Some(n) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => {
                    format!("--unified={n}")
                }
                _ => a.clone(),
            }
        })
        .collect();
    let (opts, operands) = options(
        "diff",
        &rewritten,
        "qriwbBsNcu",
        "U",
        &[
            ("brief", 'q'),
            ("recursive", 'r'),
            ("ignore-case", 'i'),
            ("ignore-all-space", 'w'),
            ("ignore-space-change", 'b'),
            ("ignore-blank-lines", 'B'),
            ("report-identical-files", 's'),
            ("new-file", 'N'),
            ("context", 'c'),
            ("unified", 'U'),
        ],
    )?;
    let o = DiffOpts {
        unified: if flag(&opts, 'u') || value(&opts, 'U').is_some() {
            Some(match value(&opts, 'U') {
                Some(v) => v
                    .parse()
                    .map_err(|_| Fail::usage(format!("diff: invalid context length '{v}'")))?,
                None => 3,
            })
        } else {
            None
        },
        context: if flag(&opts, 'c') { Some(3) } else { None },
        brief: flag(&opts, 'q'),
        report_identical: flag(&opts, 's'),
        ignore_case: flag(&opts, 'i'),
        ignore_space_change: flag(&opts, 'b'),
        ignore_all_space: flag(&opts, 'w'),
        ignore_blank_lines: flag(&opts, 'B'),
        recursive: flag(&opts, 'r'),
        new_file: flag(&opts, 'N'),
    };
    if operands.len() != 2 {
        return Err(Fail::usage(format!(
            "diff: missing operand\n{}",
            usage_line("diff")
        )));
    }
    let (left, right) = (operands[0].clone(), operands[1].clone());
    let left_dir = c.vfs.stat(&c.resolve(&left)).is_ok_and(|m| m.is_dir);
    let right_dir = c.vfs.stat(&c.resolve(&right)).is_ok_and(|m| m.is_dir);
    let mut out = String::new();
    let mut differ = false;
    if left_dir && right_dir {
        if !o.recursive {
            // Without -r, GNU compares only the entries that are plain files.
            let mut names: Vec<String> = Vec::new();
            for dir in [&left, &right] {
                for entry in c.vfs.list(&c.resolve(dir)).unwrap_or_default() {
                    if !names.contains(&entry) {
                        names.push(entry);
                    }
                }
            }
            names.sort();
            for name in names {
                let (a, b) = (format!("{left}/{name}"), format!("{right}/{name}"));
                let (text, changed) = compare_pair(c, &a, &b, &o, input)?;
                out.push_str(&text);
                differ |= changed;
            }
        } else {
            let mut names: Vec<String> = Vec::new();
            for (root, prefix) in [(&left, "a"), (&right, "b")] {
                let _ = prefix;
                for (path, _, is_dir) in walk(c, &c.resolve(root)) {
                    if is_dir {
                        continue;
                    }
                    let base = c.resolve(root);
                    if let Some(rest) = path.strip_prefix(&format!("{base}/")) {
                        if !names.contains(&rest.to_string()) {
                            names.push(rest.to_string());
                        }
                    }
                }
            }
            names.sort();
            for name in names {
                let (a, b) = (format!("{left}/{name}"), format!("{right}/{name}"));
                let (text, changed) = compare_pair(c, &a, &b, &o, input)?;
                out.push_str(&text);
                differ |= changed;
            }
        }
    } else if left_dir || right_dir {
        // `diff dir file` compares the same-named entry inside the directory.
        let (a, b) = if left_dir {
            let base = right.rsplit('/').next().unwrap_or(&right);
            (format!("{left}/{base}"), right.clone())
        } else {
            let base = left.rsplit('/').next().unwrap_or(&left);
            (left.clone(), format!("{right}/{base}"))
        };
        let (text, changed) = compare_pair(c, &a, &b, &o, input)?;
        out.push_str(&text);
        differ |= changed;
    } else {
        let (text, changed) = compare_pair(c, &left, &right, &o, input)?;
        out.push_str(&text);
        differ |= changed;
    }
    if differ {
        return Err(Fail::new(String::new(), 1).with_output(out));
    }
    Ok(out)
}
fn walk(c: &Computer, root: &str) -> Vec<(String, u64, bool)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_string()];
    while let Some(dir) = stack.pop() {
        for name in c.vfs.list(&dir).unwrap_or_default() {
            let path = if dir == "/" {
                format!("/{name}")
            } else {
                format!("{dir}/{name}")
            };
            let is_dir = c.vfs.lstat(&path).is_ok_and(|m| m.is_dir);
            out.push((path.clone(), 0, is_dir));
            if is_dir {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}
fn compare_pair(
    c: &Computer,
    a: &str,
    b: &str,
    o: &DiffOpts,
    input: &str,
) -> Result<(String, bool), Fail> {
    let missing_a = !c.vfs.exists(&c.resolve(a));
    let missing_b = !c.vfs.exists(&c.resolve(b));
    if (missing_a || missing_b) && !o.new_file {
        let name = if missing_a { a } else { b };
        return Err(Fail::op("diff", name, "No such file or directory").with_code(2));
    }
    let text_a = if missing_a {
        String::new()
    } else {
        read_text(c, "diff", a, input)?
    };
    let text_b = if missing_b {
        String::new()
    } else {
        read_text(c, "diff", b, input)?
    };
    Ok(diff_text(a, b, &text_a, &text_b, o))
}

// ---------------------------------------------------------------- cmp

fn cmp(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "cmp",
        args,
        "sl",
        "",
        &[("silent", 's'), ("quiet", 's'), ("verbose", 'l')],
    )?;
    if operands.len() < 2 {
        return Err(Fail::usage(format!(
            "cmp: missing operand\n{}",
            usage_line("cmp")
        )));
    }
    let a = read_bytes(c, "cmp", &operands[0], input)?;
    let b = read_bytes(c, "cmp", &operands[1], input)?;
    let quiet = flag(&opts, 's');
    if flag(&opts, 'l') {
        let mut out = String::new();
        let mut differ = false;
        for i in 0..a.len().min(b.len()) {
            if a[i] != b[i] {
                differ = true;
                out.push_str(&format!("{:>6} {:o} {:o}\n", i + 1, a[i], b[i]));
            }
        }
        if a.len() != b.len() {
            let (short, name) = if a.len() < b.len() {
                (a.len(), &operands[0])
            } else {
                (b.len(), &operands[1])
            };
            return Err(
                Fail::new(format!("cmp: EOF on {name} after byte {short}"), 1).with_output(out),
            );
        }
        if differ {
            return Err(Fail::new(String::new(), 1).with_output(out));
        }
        return Ok(out);
    }
    for i in 0..a.len().min(b.len()) {
        if a[i] != b[i] {
            let line = a[..i].iter().filter(|x| **x == b'\n').count() + 1;
            // GNU writes the "differ" line to stdout, not stderr.
            let message = if quiet {
                String::new()
            } else {
                format!(
                    "{} {} differ: byte {}, line {line}\n",
                    operands[0],
                    operands[1],
                    i + 1
                )
            };
            return Err(Fail::new(String::new(), 1).with_output(message));
        }
    }
    if a.len() != b.len() {
        let (short, name) = if a.len() < b.len() {
            (a.len(), &operands[0])
        } else {
            (b.len(), &operands[1])
        };
        let message = if quiet {
            String::new()
        } else {
            format!("cmp: EOF on {name} after byte {short}")
        };
        return Err(Fail::new(message, 1));
    }
    Ok(String::new())
}

// ---------------------------------------------------------------- digests

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(char::from(B64[(n >> 18) as usize & 63]));
        out.push(char::from(B64[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(B64[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(B64[n as usize & 63])
        } else {
            '='
        });
    }
    out
}
fn base64_decode(text: &str, strict: bool) -> Result<Vec<u8>, Fail> {
    let mut bits = 0u32;
    let mut have = 0u32;
    let mut out = Vec::new();
    for ch in text.chars() {
        if ch == '=' {
            break;
        }
        if ch.is_whitespace() {
            continue;
        }
        let Some(v) = B64.iter().position(|b| char::from(*b) == ch) else {
            if strict {
                return Err(Fail::new("base64: invalid input", 1));
            }
            continue;
        };
        bits = (bits << 6) | v as u32;
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push(((bits >> have) & 0xFF) as u8);
        }
    }
    Ok(out)
}
fn base64(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "base64",
        args,
        "di",
        "w",
        &[("decode", 'd'), ("ignore-garbage", 'i'), ("wrap", 'w')],
    )?;
    let name = operands.first().map_or("-", String::as_str);
    if flag(&opts, 'd') {
        let text = read_text(c, "base64", name, input)?;
        let bytes = base64_decode(&text, !flag(&opts, 'i'))?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    let bytes = read_bytes(c, "base64", name, input)?;
    let encoded = base64_encode(&bytes);
    let wrap = match value(&opts, 'w') {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| Fail::usage(format!("base64: invalid wrap size: '{v}'")))?,
        None => 76,
    };
    if wrap == 0 {
        return Ok(format!("{encoded}\n"));
    }
    let mut out = String::new();
    let chars: Vec<char> = encoded.chars().collect();
    for chunk in chars.chunks(wrap) {
        out.push_str(&chunk.iter().collect::<String>());
        out.push('\n');
    }
    if out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}
/// MD5, implemented here because the world must hash without a host crate that would
/// pull in platform code; it is a pure function of its bytes.
fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((f64::from(i as u32 + 1).sin().abs()) * 4_294_967_296.0) as u32)
        .collect();
    let mut state = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_le_bytes());
    for block in message.chunks(64) {
        let m: Vec<u32> = block
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let (mut a, mut b, mut c2, mut d) = (state[0], state[1], state[2], state[3]);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c2) | (!b & d), i),
                1 => ((d & b) | (!d & c2), (5 * i + 1) % 16),
                2 => (b ^ c2 ^ d, (3 * i + 5) % 16),
                _ => (c2 ^ (b | !d), (7 * i) % 16),
            };
            let tmp = d;
            d = c2;
            c2 = b;
            let sum = a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = tmp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c2);
        state[3] = state[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A82_7999u32),
                1 => (b ^ c ^ d, 0x6ED9_EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}
fn digest(cmd: &str, data: &[u8]) -> String {
    let bytes: Vec<u8> = match cmd {
        "md5sum" => md5(data).to_vec(),
        "sha1sum" => sha1(data).to_vec(),
        _ => {
            use sha2::Digest;
            sha2::Sha256::digest(data).to_vec()
        }
    };
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn checksum(c: &Computer, cmd: &str, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        cmd,
        args,
        "cbt",
        "",
        &[("check", 'c'), ("binary", 'b'), ("text", 't')],
    )?;
    if flag(&opts, 'c') {
        let mut out = String::new();
        let mut bad = 0;
        for list in if operands.is_empty() {
            vec![String::from("-")]
        } else {
            operands.clone()
        } {
            let text = read_text(c, cmd, &list, input)?;
            for line in lines_of(&text) {
                let Some((want, name)) = line.split_once(char::is_whitespace) else {
                    continue;
                };
                let name = name.trim_start_matches([' ', '*']);
                match read_bytes(c, cmd, name, input) {
                    Ok(bytes) if digest(cmd, &bytes) == want.trim() => {
                        out.push_str(&format!("{name}: OK\n"));
                    }
                    Ok(_) => {
                        out.push_str(&format!("{name}: FAILED\n"));
                        bad += 1;
                    }
                    Err(_) => {
                        out.push_str(&format!("{cmd}: {name}: No such file or directory\n"));
                        out.push_str(&format!("{name}: FAILED open or read\n"));
                        bad += 1;
                    }
                }
            }
        }
        if bad > 0 {
            return Err(Fail::new(
                format!("{cmd}: WARNING: {bad} computed checksum did NOT match"),
                1,
            )
            .raw()
            .with_output(out));
        }
        return Ok(out);
    }
    let names: Vec<String> = if operands.is_empty() {
        vec![String::from("-")]
    } else {
        operands.clone()
    };
    let mut out = String::new();
    for name in &names {
        let bytes = read_bytes(c, cmd, name, input)?;
        out.push_str(&format!("{}  {name}\n", digest(cmd, &bytes)));
    }
    Ok(out)
}

// ---------------------------------------------------------------- dumps

fn printable(b: u8) -> char {
    if (0x20..0x7f).contains(&b) {
        char::from(b)
    } else {
        '.'
    }
}
fn xxd(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "xxd",
        args,
        "pur",
        "clsg",
        &[
            ("plain", 'p'),
            ("uppercase", 'u'),
            ("revert", 'r'),
            ("cols", 'c'),
            ("len", 'l'),
            ("seek", 's'),
            ("group", 'g'),
        ],
    )?;
    let cols = match value(&opts, 'c') {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| Fail::usage(format!("xxd: invalid number of columns: '{v}'")))?
            .max(1),
        None => {
            if flag(&opts, 'p') {
                30
            } else {
                16
            }
        }
    };
    let group = match value(&opts, 'g') {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| Fail::usage(format!("xxd: invalid group size: '{v}'")))?,
        None => 2,
    };
    let name = operands.first().map_or("-", String::as_str);
    if flag(&opts, 'r') {
        let text = read_text(c, "xxd", name, input)?;
        let mut out = Vec::new();
        for line in lines_of(&text) {
            let body = if flag(&opts, 'p') {
                line
            } else {
                line.split_once(':').map_or(line, |(_, rest)| rest)
            };
            let hex: Vec<char> = body
                .chars()
                .take_while(|_| true)
                .filter(|ch| ch.is_ascii_hexdigit() || ch.is_whitespace())
                .collect();
            let compact: String = hex.iter().filter(|ch| !ch.is_whitespace()).collect();
            let limit = if flag(&opts, 'p') {
                compact.len() / 2
            } else {
                cols.min(compact.len() / 2)
            };
            let bytes: Vec<u8> = compact.as_bytes()[..limit * 2]
                .chunks(2)
                .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
                .collect();
            out.extend(bytes);
        }
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let mut bytes = read_bytes(c, "xxd", name, input)?;
    if let Some(skip) = value(&opts, 's') {
        let n: usize = skip
            .parse()
            .map_err(|_| Fail::usage(format!("xxd: invalid seek '{skip}'")))?;
        bytes = bytes[n.min(bytes.len())..].to_vec();
    }
    if let Some(limit) = value(&opts, 'l') {
        let n: usize = limit
            .parse()
            .map_err(|_| Fail::usage(format!("xxd: invalid length '{limit}'")))?;
        bytes.truncate(n);
    }
    let hex = |b: u8| {
        if flag(&opts, 'u') {
            format!("{b:02X}")
        } else {
            format!("{b:02x}")
        }
    };
    let mut out = String::new();
    if flag(&opts, 'p') {
        for chunk in bytes.chunks(cols) {
            out.push_str(&chunk.iter().map(|b| hex(*b)).collect::<String>());
            out.push('\n');
        }
        if bytes.is_empty() {
            return Ok(String::new());
        }
        return Ok(out);
    }
    for (i, chunk) in bytes.chunks(cols).enumerate() {
        let mut body = String::new();
        for (k, b) in chunk.iter().enumerate() {
            if group > 0 && k > 0 && k % group == 0 {
                body.push(' ');
            }
            body.push_str(&hex(*b));
        }
        let width = cols * 2
            + if group > 0 {
                cols.div_ceil(group) - 1
            } else {
                0
            };
        let ascii: String = chunk.iter().map(|b| printable(*b)).collect();
        out.push_str(&format!(
            "{:08x}: {body:<width$}  {ascii}\n",
            i * cols,
            width = width
        ));
    }
    Ok(out)
}
fn od(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "od",
        args,
        "cbxdov",
        "AtNj",
        &[
            ("address-radix", 'A'),
            ("format", 't'),
            ("read-bytes", 'N'),
            ("skip-bytes", 'j'),
            ("output-duplicates", 'v'),
        ],
    )?;
    let radix = value(&opts, 'A').unwrap_or("o").to_string();
    if !matches!(radix.as_str(), "o" | "d" | "x" | "n") {
        return Err(Fail::usage(format!(
            "od: invalid output address radix '{radix}'; it must be one character from [doxn]"
        )));
    }
    let kind = match value(&opts, 't') {
        Some(t) => match t {
            "c" => 'c',
            "a" => 'c',
            "x1" => 'x',
            "o1" => 'b',
            "d1" => 'd',
            "o2" => 'o',
            other => {
                return Err(Fail::usage(format!(
                    "od: this world models -t c, a, x1, o1, d1 and o2, not '{other}'"
                )))
            }
        },
        None => {
            if flag(&opts, 'c') {
                'c'
            } else if flag(&opts, 'b') {
                'b'
            } else if flag(&opts, 'x') {
                'x'
            } else if flag(&opts, 'd') {
                'd'
            } else {
                'o'
            }
        }
    };
    let mut bytes = read_bytes(c, "od", operands.first().map_or("-", String::as_str), input)?;
    if let Some(skip) = value(&opts, 'j') {
        let n: usize = skip
            .parse()
            .map_err(|_| Fail::usage(format!("od: invalid skip '{skip}'")))?;
        bytes = bytes[n.min(bytes.len())..].to_vec();
    }
    if let Some(limit) = value(&opts, 'N') {
        let n: usize = limit
            .parse()
            .map_err(|_| Fail::usage(format!("od: invalid byte count '{limit}'")))?;
        bytes.truncate(n);
    }
    let address = |offset: usize| -> String {
        match radix.as_str() {
            "d" => format!("{offset:07}"),
            "x" => format!("{offset:06x}"),
            "n" => String::new(),
            _ => format!("{offset:07o}"),
        }
    };
    let mut out = String::new();
    let mut previous: Option<Vec<u8>> = None;
    let mut starred = false;
    for (i, chunk) in bytes.chunks(16).enumerate() {
        if !flag(&opts, 'v') && previous.as_deref() == Some(chunk) && chunk.len() == 16 {
            if !starred {
                out.push_str("*\n");
                starred = true;
            }
            continue;
        }
        starred = false;
        previous = Some(chunk.to_vec());
        let body = match kind {
            'c' => chunk
                .iter()
                .map(|b| {
                    let text = match b {
                        0 => String::from("\\0"),
                        7 => String::from("\\a"),
                        8 => String::from("\\b"),
                        9 => String::from("\\t"),
                        10 => String::from("\\n"),
                        11 => String::from("\\v"),
                        12 => String::from("\\f"),
                        13 => String::from("\\r"),
                        _ if (0x20..0x7f).contains(b) => char::from(*b).to_string(),
                        _ => format!("{b:03o}"),
                    };
                    format!("{text:>4}")
                })
                .collect::<String>(),
            'b' => chunk
                .iter()
                .map(|b| format!(" {b:03o}"))
                .collect::<String>(),
            'x' => chunk
                .iter()
                .map(|b| format!(" {b:02x}"))
                .collect::<String>(),
            'd' => chunk.iter().map(|b| format!(" {b:3}")).collect::<String>(),
            _ => chunk
                .chunks(2)
                .map(|pair| {
                    let v = u16::from(pair[0]) | (u16::from(*pair.get(1).unwrap_or(&0)) << 8);
                    format!(" {v:06o}")
                })
                .collect::<String>(),
        };
        out.push_str(&format!("{}{body}\n", address(i * 16)));
    }
    if radix != "n" {
        out.push_str(&format!("{}\n", address(bytes.len())));
    }
    Ok(out)
}
fn hexdump(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "hexdump",
        args,
        "Ccbxdov",
        "ns",
        &[
            ("canonical", 'C'),
            ("one-byte-char", 'c'),
            ("one-byte-octal", 'b'),
            ("two-bytes-hex", 'x'),
            ("length", 'n'),
            ("skip", 's'),
            ("no-squeezing", 'v'),
        ],
    )?;
    let mut bytes = read_bytes(
        c,
        "hexdump",
        operands.first().map_or("-", String::as_str),
        input,
    )?;
    if let Some(skip) = value(&opts, 's') {
        let n: usize = skip
            .parse()
            .map_err(|_| Fail::usage(format!("hexdump: invalid skip '{skip}'")))?;
        bytes = bytes[n.min(bytes.len())..].to_vec();
    }
    if let Some(limit) = value(&opts, 'n') {
        let n: usize = limit
            .parse()
            .map_err(|_| Fail::usage(format!("hexdump: invalid length '{limit}'")))?;
        bytes.truncate(n);
    }
    let mut out = String::new();
    let mut previous: Option<Vec<u8>> = None;
    let mut starred = false;
    for (i, chunk) in bytes.chunks(16).enumerate() {
        if !flag(&opts, 'v') && previous.as_deref() == Some(chunk) && chunk.len() == 16 {
            if !starred {
                out.push_str("*\n");
                starred = true;
            }
            continue;
        }
        starred = false;
        previous = Some(chunk.to_vec());
        if flag(&opts, 'C') {
            let left: String = chunk[..chunk.len().min(8)]
                .iter()
                .map(|b| format!("{b:02x} "))
                .collect();
            let right: String = chunk[chunk.len().min(8)..]
                .iter()
                .map(|b| format!("{b:02x} "))
                .collect();
            let ascii: String = chunk.iter().map(|b| printable(*b)).collect();
            out.push_str(&format!("{:08x}  {left:<24}{right:<25}|{ascii}|\n", i * 16));
        } else if flag(&opts, 'c') {
            let body: String = chunk
                .iter()
                .map(|b| {
                    let t = match b {
                        0 => String::from("\\0"),
                        9 => String::from("\\t"),
                        10 => String::from("\\n"),
                        _ if (0x20..0x7f).contains(b) => char::from(*b).to_string(),
                        _ => format!("{b:03o}"),
                    };
                    format!("{t:>4}")
                })
                .collect();
            out.push_str(&format!("{:07x}{body}\n", i * 16));
        } else if flag(&opts, 'b') {
            let body: String = chunk.iter().map(|b| format!(" {b:03o}")).collect();
            out.push_str(&format!("{:07x}{body}\n", i * 16));
        } else {
            // Two-byte words: octal by default and with -o, hex with -x, decimal with -d.
            let body: String = chunk
                .chunks(2)
                .map(|pair| {
                    let v = u16::from(pair[0]) | (u16::from(*pair.get(1).unwrap_or(&0)) << 8);
                    if flag(&opts, 'x') {
                        format!(" {v:04x}")
                    } else if flag(&opts, 'd') {
                        format!(" {v:05}")
                    } else {
                        format!(" {v:06o}")
                    }
                })
                .collect();
            out.push_str(&format!("{:07x}{body}\n", i * 16));
        }
    }
    out.push_str(&format!(
        "{:0width$x}\n",
        bytes.len(),
        width = if flag(&opts, 'C') { 8 } else { 7 }
    ));
    Ok(out)
}

// ---------------------------------------------------------------- file

/// The magic table covers the formats this world actually writes. Anything else is
/// classified honestly as text or data rather than guessed at.
fn magic(c: &Computer, bytes: &[u8], path: &str) -> String {
    if bytes.is_empty() {
        return String::from("empty");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (w, h) = png_size(bytes);
        let animated = find(bytes, b"acTL").is_some();
        let colour = bytes.get(25).copied().unwrap_or(6);
        let kind = match colour {
            0 => "grayscale",
            2 => "RGB",
            3 => "colormap",
            4 => "gray+alpha",
            _ => "RGBA",
        };
        let depth = bytes.get(24).copied().unwrap_or(8);
        return if animated {
            format!("PNG image data, {w} x {h}, {depth}-bit/color {kind}, non-interlaced, APNG")
        } else {
            format!("PNG image data, {w} x {h}, {depth}-bit/color {kind}, non-interlaced")
        };
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return String::from("JPEG image data, JFIF standard");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return String::from("GIF image data");
    }
    if bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WAVE" {
        return String::from("RIFF (little-endian) data, WAVE audio");
    }
    if bytes.starts_with(b"SQLite format 3\0") {
        return String::from("SQLite 3.x database");
    }
    if bytes.starts_with(b"%PDF-") {
        let version = String::from_utf8_lossy(&bytes[5..bytes.len().min(8)]).to_string();
        return format!("PDF document, version {version}");
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        if find(bytes, b"xl/workbook.xml").is_some() {
            return String::from("Microsoft Excel 2007+");
        }
        if find(bytes, b"word/document.xml").is_some() {
            return String::from("Microsoft Word 2007+");
        }
        if find(bytes, b"ppt/presentation.xml").is_some() {
            return String::from("Microsoft PowerPoint 2007+");
        }
        if find(bytes, b"opendocument.spreadsheet").is_some() {
            return String::from("OpenDocument Spreadsheet");
        }
        if find(bytes, b"opendocument.text").is_some() {
            return String::from("OpenDocument Text");
        }
        return String::from("Zip archive data");
    }
    if bytes.starts_with(b"\x7fELF") {
        // Nothing in this world produces one; if a consumer copies one in, say so.
        return String::from("ELF binary (this world runs no native executables)");
    }
    if bytes.starts_with(b"#!") {
        let line = String::from_utf8_lossy(&bytes[..bytes.len().min(128)]);
        let interpreter = line.lines().next().unwrap_or("").trim_start_matches("#!");
        return format!(
            "{} script, ASCII text executable",
            interpreter.split('/').next_back().unwrap_or("shell").trim()
        );
    }
    let text = std::str::from_utf8(bytes);
    match text {
        Ok(s)
            if !s
                .chars()
                .any(|ch| ch.is_control() && !"\n\r\t\u{c}".contains(ch)) =>
        {
            let ascii = s.is_ascii();
            let json = s.trim_start().starts_with('{') || s.trim_start().starts_with('[');
            let name = path.rsplit('.').next().unwrap_or("");
            let flavour = if json && serde_json::from_str::<serde_json::Value>(s).is_ok() {
                "JSON text data"
            } else if name == "csv" {
                "CSV text"
            } else if ascii {
                "ASCII text"
            } else {
                "Unicode text, UTF-8 text"
            };
            if s.ends_with('\n') || s.is_empty() {
                flavour.to_string()
            } else {
                format!("{flavour}, with no line terminators")
            }
        }
        _ => {
            let _ = c;
            String::from("data")
        }
    }
}
fn png_size(bytes: &[u8]) -> (u32, u32) {
    if bytes.len() < 24 {
        return (0, 0);
    }
    (
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
    )
}
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
fn file(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, operands) = options(
        "file",
        args,
        "bihL",
        "",
        &[
            ("brief", 'b'),
            ("mime-type", 'i'),
            ("mime", 'i'),
            ("no-dereference", 'h'),
            ("dereference", 'L'),
        ],
    )?;
    if operands.is_empty() {
        return Err(Fail::usage(format!(
            "file: missing operand\n{}",
            usage_line("file")
        )));
    }
    let mut out = String::new();
    for name in &operands {
        let path = c.resolve(name);
        let description = match c.vfs.lstat(&path) {
            Err(e) => {
                out.push_str(&format!(
                    "{name}: cannot open `{name}' ({})\n",
                    crate::shell::vfs_reason(&e)
                ));
                continue;
            }
            Ok(meta) if meta.is_symlink && !flag(&opts, 'L') => {
                let target = c.vfs.read_link(&path).unwrap_or_default();
                format!("symbolic link to {target}")
            }
            Ok(meta) if meta.is_dir => String::from("directory"),
            Ok(_) => {
                let bytes = read_bytes(c, "file", name, input)?;
                magic(c, &bytes, name)
            }
        };
        let description = if flag(&opts, 'i') {
            mime_of(&description).to_string()
        } else {
            description
        };
        if flag(&opts, 'b') {
            out.push_str(&format!("{description}\n"));
        } else {
            out.push_str(&format!("{name}: {description}\n"));
        }
    }
    Ok(out)
}
fn mime_of(description: &str) -> &'static str {
    if description.starts_with("PNG") {
        "image/png"
    } else if description.starts_with("JPEG") {
        "image/jpeg"
    } else if description.starts_with("GIF") {
        "image/gif"
    } else if description.contains("WAVE") {
        "audio/x-wav"
    } else if description.starts_with("SQLite") {
        "application/vnd.sqlite3"
    } else if description.starts_with("PDF") {
        "application/pdf"
    } else if description.starts_with("Microsoft Excel") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else if description.starts_with("Zip") {
        "application/zip"
    } else if description == "directory" {
        "inode/directory"
    } else if description == "empty" {
        "inode/x-empty"
    } else if description.contains("text") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

// ---------------------------------------------------------------- dispatch

pub(crate) fn run(
    c: &mut Computer,
    cmd: &str,
    args: &[String],
    input: &str,
) -> Option<Result<String, Fail>> {
    Some(match cmd {
        "diff" => diff(c, args, input),
        "cmp" => cmp(c, args, input),
        "base64" => base64(c, args, input),
        "md5sum" | "sha1sum" | "sha256sum" => checksum(c, cmd, args, input),
        "xxd" => xxd(c, args, input),
        "od" => od(c, args, input),
        "hexdump" => hexdump(c, args, input),
        "file" => file(c, args, input),
        _ => return None,
    })
}
