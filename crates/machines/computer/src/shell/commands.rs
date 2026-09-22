use super::*;

/// Simulated wall clock. Tick 0 is 09:00:00 UTC on Thursday 17 September 2026 — the
/// same origin the desktop clock uses, so `date` and the GUI never disagree.
pub const EPOCH_UNIX_SECONDS: u64 = 1_789_635_600;
const WEEKDAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_NAME: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_NAME: [&str; 12] = [
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub year: u64,
    pub month: u64,
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
    pub second: u64,
    pub micro: u64,
    /// 0 is Sunday.
    pub weekday: u64,
    pub unix: u64,
}
/// Gregorian civil date from a simulated microsecond tick; no host clock is consulted.
pub fn clock(tick: u64) -> Clock {
    let unix = EPOCH_UNIX_SECONDS + tick / 1_000_000;
    let (days, rest) = (unix / 86_400, unix % 86_400);
    // Hinnant's civil-from-days: a March-based year puts the leap day last.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Clock {
        year: (yoe + era * 400 + i64::from(month <= 2)) as u64,
        month: month as u64,
        day: day as u64,
        hour: rest / 3600,
        minute: rest % 3600 / 60,
        second: rest % 60,
        micro: tick % 1_000_000,
        weekday: (days + 4) % 7,
        unix,
    }
}
impl Clock {
    /// `2026-09-17 09:00:00`, the stamp `stat` and `uptime -s` print.
    pub(crate) fn stamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
    /// `Sep 17 09:00`, the `ls -l` column.
    pub(crate) fn short(&self) -> String {
        format!(
            "{} {:2} {:02}:{:02}",
            MONTH_ABBR[self.month as usize - 1],
            self.day,
            self.hour,
            self.minute
        )
    }
}
/// GNU-style abbreviated size: exact under 1 KiB, one decimal under 10 units.
pub(crate) fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        bytes.to_string()
    } else if value < 9.95 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{}{}", value.ceil() as u64, UNITS[unit])
    }
}
/// `drwxr-xr-t` from the stored mode; setuid/setgid/sticky bits are honoured.
pub(crate) fn mode_string(mode: u16, is_dir: bool, is_symlink: bool) -> String {
    let kind = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let mut out = String::from(kind);
    for (shift, special, low, high) in [
        (6, 0o4000, 's', 'S'),
        (3, 0o2000, 's', 'S'),
        (0, 0o1000, 't', 'T'),
    ] {
        let bits = (mode >> shift) & 7;
        out.push(if bits & 4 != 0 { 'r' } else { '-' });
        out.push(if bits & 2 != 0 { 'w' } else { '-' });
        out.push(if mode & special != 0 {
            if bits & 1 != 0 {
                low
            } else {
                high
            }
        } else if bits & 1 != 0 {
            'x'
        } else {
            '-'
        });
    }
    out
}
/// Bytes a node occupies, assuming a 4 KiB allocation unit; the VFS has no allocator,
/// so this is a stated convention rather than a measurement.
pub(crate) const BLOCK: u64 = 4096;
pub(crate) fn allocated(size: u64) -> u64 {
    size.div_ceil(BLOCK) * BLOCK
}
/// Bytes a node reports: the VFS stores a child count for directories, but every tool
/// that prints a size means the allocation unit.
pub(crate) fn apparent(m: &crate::Metadata) -> u64 {
    if m.is_dir {
        BLOCK
    } else {
        m.size as u64
    }
}
/// Every node beneath `root` (inclusive) with its apparent size; directories are
/// reported as one allocation unit. Walks the VFS only.
pub(crate) fn walk_tree(c: &Computer, root: &str) -> Vec<(String, u64, bool)> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![root.trim_end_matches('/').to_string()];
    while let Some(path) = stack.pop() {
        let path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };
        let Ok(meta) = c.vfs.lstat(&path) else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        if meta.is_dir {
            out.push((path.clone(), BLOCK, true));
            for name in c.vfs.list(&path).unwrap_or_default() {
                stack.push(format!("{}/{name}", path.trim_end_matches('/')));
            }
        } else {
            out.push((path, meta.size as u64, false));
        }
    }
    out.sort();
    out
}

/// Option parser shared by the file utilities. Clusters (`-la`) split, long names map
/// to their short letter, and anything unlisted is a usage failure: a flag this world
/// cannot honour must never be silently dropped.
pub(crate) type Parsed = (Vec<(char, String)>, Vec<String>);
/// The one-line synopsis printed under a refused flag, exactly as GNU coreutils does.
/// A command with no entry gets the generic form; the point is that the caller always
/// sees what the command *does* accept next to what it refused.
pub(crate) fn usage_line(name: &str) -> String {
    let body = match name {
        "awk" => "awk [-F fs] [-v var=value] ['prog' | -f progfile] [file ...]",
        "sed" => "sed [-n] [-E] [-i[SUFFIX]] [-e script] [-f script-file] [file ...]",
        "xargs" => "xargs [-0rt] [-d delim] [-I replace-str] [-n max-args] [-P max-procs] [command [args...]]",
        "cut" => "cut OPTION... [FILE]...",
        "sort" => "sort [OPTION]... [FILE]...",
        "uniq" => "uniq [OPTION]... [INPUT [OUTPUT]]",
        "head" | "tail" => "head [OPTION]... [FILE]...",
        "wc" => "wc [OPTION]... [FILE]...",
        "tr" => "tr [OPTION]... SET1 [SET2]",
        "paste" => "paste [OPTION]... [FILE]...",
        "join" => "join [OPTION]... FILE1 FILE2",
        "comm" => "comm [OPTION]... FILE1 FILE2",
        "diff" => "diff [OPTION]... FILES",
        "tee" => "tee [OPTION]... [FILE]...",
        "nl" => "nl [OPTION]... [FILE]...",
        "fold" => "fold [OPTION]... [FILE]...",
        "expand" | "unexpand" => "expand [OPTION]... [FILE]...",
        "shuf" => "shuf [OPTION]... [FILE]",
        "seq" => "seq [OPTION]... LAST",
        "basename" => "basename NAME [SUFFIX] or: basename OPTION... NAME...",
        "dirname" => "dirname [OPTION] NAME...",
        "realpath" => "realpath [OPTION]... FILE...",
        "readlink" => "readlink [OPTION]... FILE...",
        "base64" => "base64 [OPTION]... [FILE]",
        "cmp" => "cmp [OPTION]... FILE1 [FILE2]",
        "split" => "split [OPTION]... [FILE [PREFIX]]",
        "strings" => "strings [OPTION]... [FILE]...",
        "file" => "file [OPTION]... FILE...",
        "od" => "od [OPTION]... [FILE]...",
        "xxd" => "xxd [OPTION]... [FILE]",
        "hexdump" => "hexdump [OPTION]... [FILE]...",
        "md5sum" | "sha1sum" | "sha256sum" => "md5sum [OPTION]... [FILE]...",
        "ls" => "ls [OPTION]... [FILE]...",
        "grep" => "grep [OPTION]... PATTERNS [FILE]...",
        "cp" => "cp [OPTION]... SOURCE... DIRECTORY",
        "mv" => "mv [OPTION]... SOURCE... DIRECTORY",
        "rm" => "rm [OPTION]... [FILE]...",
        "mkdir" => "mkdir [OPTION]... DIRECTORY...",
        "find" => "find [-H] [-L] [-P] [path...] [expression]",
        _ => return format!("Usage: {name} [OPTION]... [FILE]..."),
    };
    format!("Usage: {body}")
}
/// `cmd: invalid option -- 'x'` plus the synopsis, GNU's exact shape, status 2.
pub(crate) fn invalid_option(name: &str, ch: char) -> Fail {
    Fail::usage(format!(
        "{name}: invalid option -- '{ch}'\n{}",
        usage_line(name)
    ))
}
/// `cmd: unrecognized option '--x'` plus the synopsis, status 2.
pub(crate) fn unrecognized_option(name: &str, long: &str) -> Fail {
    Fail::usage(format!(
        "{name}: unrecognized option '--{long}'\n{}",
        usage_line(name)
    ))
}
pub(crate) fn missing_argument(name: &str, spelling: &str) -> Fail {
    Fail::usage(format!(
        "{name}: option requires an argument -- '{spelling}'\n{}",
        usage_line(name)
    ))
}
pub(crate) fn options(
    name: &str,
    args: &[String],
    allowed: &str,
    valued: &str,
    long: &[(&str, char)],
) -> Result<Parsed, Fail> {
    let mut flags = Vec::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            operands.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(rest) = arg.strip_prefix("--").filter(|s| !s.is_empty()) {
            let (key, inline) = match rest.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (rest, None),
            };
            let ch = long
                .iter()
                .find(|(n, _)| *n == key)
                .map(|(_, c)| *c)
                .ok_or_else(|| unrecognized_option(name, key))?;
            if valued.contains(ch) {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        args.get(i)
                            .cloned()
                            .ok_or_else(|| missing_argument(name, key))?
                    }
                };
                flags.push((ch, value));
            } else if inline.is_some() {
                return Err(Fail::usage(format!(
                    "{name}: option '--{key}' doesn't allow an argument\n{}",
                    usage_line(name)
                )));
            } else {
                flags.push((ch, String::new()));
            }
            i += 1;
            continue;
        }
        if arg.len() > 1 && arg.starts_with('-') {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let ch = chars[j];
                if valued.contains(ch) {
                    let tail: String = chars[j + 1..].iter().collect();
                    let value = if tail.is_empty() {
                        i += 1;
                        args.get(i)
                            .cloned()
                            .ok_or_else(|| missing_argument(name, &ch.to_string()))?
                    } else {
                        tail
                    };
                    flags.push((ch, value));
                    j = chars.len();
                } else if allowed.contains(ch) {
                    flags.push((ch, String::new()));
                    j += 1;
                } else {
                    return Err(invalid_option(name, ch));
                }
            }
            i += 1;
            continue;
        }
        operands.push(arg.clone());
        i += 1;
    }
    Ok((flags, operands))
}
pub(crate) fn flag(flags: &[(char, String)], f: char) -> bool {
    flags.iter().any(|(k, _)| *k == f)
}
pub(crate) fn value(flags: &[(char, String)], f: char) -> Option<&str> {
    flags
        .iter()
        .rev()
        .find(|(k, _)| *k == f)
        .map(|(_, v)| v.as_str())
}
/// `date +FORMAT`. Unknown conversions fail rather than being copied through, so a
/// caller never mistakes an unimplemented field for a literal.
pub(crate) fn strftime(format: &str, now: &Clock) -> Result<String, Fail> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(match chars[i + 1] {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            i += 2;
            continue;
        }
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let spec = *chars
            .get(i + 1)
            .ok_or_else(|| Fail::usage("date: trailing `%` in format".to_string()))?;
        let (y, mo, d) = (now.year, now.month, now.day);
        let (h, mi, s) = (now.hour, now.minute, now.second);
        match spec {
            '%' => out.push('%'),
            'Y' => out.push_str(&format!("{y:04}")),
            'y' => out.push_str(&format!("{:02}", y % 100)),
            'm' => out.push_str(&format!("{mo:02}")),
            'd' => out.push_str(&format!("{d:02}")),
            'e' => out.push_str(&format!("{d:2}")),
            'H' => out.push_str(&format!("{h:02}")),
            'M' => out.push_str(&format!("{mi:02}")),
            'S' => out.push_str(&format!("{s:02}")),
            'N' => out.push_str(&format!("{:09}", now.micro * 1000)),
            's' => out.push_str(&now.unix.to_string()),
            'F' => out.push_str(&format!("{y:04}-{mo:02}-{d:02}")),
            'T' => out.push_str(&format!("{h:02}:{mi:02}:{s:02}")),
            'D' => out.push_str(&format!("{:02}/{d:02}/{:02}", mo, y % 100)),
            'a' => out.push_str(WEEKDAY_ABBR[now.weekday as usize]),
            'A' => out.push_str(WEEKDAY_NAME[now.weekday as usize]),
            'b' | 'h' => out.push_str(MONTH_ABBR[mo as usize - 1]),
            'B' => out.push_str(MONTH_NAME[mo as usize - 1]),
            'u' => out.push_str(&(if now.weekday == 0 { 7 } else { now.weekday }).to_string()),
            'w' => out.push_str(&now.weekday.to_string()),
            'Z' => out.push_str("UTC"),
            'z' => out.push_str("+0000"),
            other => {
                return Err(Fail::usage(format!(
                    "date: unsupported conversion `%{other}`"
                )))
            }
        }
        i += 2;
    }
    Ok(out)
}
/// Trailing type marker for `ls -F`; executability comes from the stored mode.
pub(crate) fn classify(meta: &crate::Metadata) -> char {
    if meta.is_symlink {
        '@'
    } else if meta.is_dir {
        '/'
    } else if meta.mode & 0o111 != 0 {
        '*'
    } else {
        ' '
    }
}

/// PowerShell spells switches with one dash and a whole word; fold the ones the
/// file commands accept onto their POSIX letters before parsing.
pub(crate) fn powershell_switches(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|a| match a.as_str() {
            "-Recurse" => "-r".to_string(),
            "-Force" => "-f".to_string(),
            other => other.to_string(),
        })
        .collect()
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_date(args: &[String], t: u64) -> Result<String, Fail> {
    let now = clock(t);
    let mut chosen = None;
    for arg in args {
        match arg.strip_prefix('+') {
            Some(f) => chosen = Some(f.to_string()),
            None if matches!(arg.as_str(), "-u" | "--utc" | "--universal") => {}
            None => {
                return Err(Fail::usage(format!(
                    "date: unsupported option `{arg}`; the simulated clock is UTC and read-only"
                )))
            }
        }
    }
    match chosen {
        Some(f) => Ok(format!("{}\n", strftime(&f, &now)?)),
        None => Ok(format!(
            "{} {} {:2} {:02}:{:02}:{:02} UTC {:04}\n",
            WEEKDAY_ABBR[now.weekday as usize],
            MONTH_ABBR[now.month as usize - 1],
            now.day,
            now.hour,
            now.minute,
            now.second,
            now.year
        )),
    }
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_grep(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, mut rest) = options(
        "grep",
        args,
        "ivnclLFEGqshHwxrRo",
        "eABC",
        &[
            ("ignore-case", 'i'),
            ("invert-match", 'v'),
            ("line-number", 'n'),
            ("count", 'c'),
            ("files-with-matches", 'l'),
            ("files-without-match", 'L'),
            ("fixed-strings", 'F'),
            ("extended-regexp", 'E'),
            ("basic-regexp", 'G'),
            ("regexp", 'e'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("no-messages", 's'),
            ("no-filename", 'h'),
            ("with-filename", 'H'),
            ("word-regexp", 'w'),
            ("line-regexp", 'x'),
            ("recursive", 'r'),
            ("only-matching", 'o'),
            ("after-context", 'A'),
            ("before-context", 'B'),
            ("context", 'C'),
        ],
    )?;
    let count = |letter: char| -> Result<Option<usize>, Fail> {
        match value(&opts, letter) {
            Some(v) => v
                .parse()
                .map(Some)
                .map_err(|_| Fail::usage(format!("grep: option `-{letter}` expects a number"))),
            None => Ok(None),
        }
    };
    let both = count('C')?;
    let (after, before) = (
        count('A')?.or(both).unwrap_or(0),
        count('B')?.or(both).unwrap_or(0),
    );
    // Asking for context at all switches on the `--` group separator, even at 0.
    let context = ['A', 'B', 'C'].iter().any(|f| value(&opts, *f).is_some());
    let mut patterns: Vec<String> = opts
        .iter()
        .filter(|(k, _)| *k == 'e')
        .map(|(_, v)| v.clone())
        .collect();
    if patterns.is_empty() {
        if rest.is_empty() {
            return Err(Fail::usage("grep: missing pattern"));
        }
        patterns.push(rest.remove(0));
    }
    let recursive = flag(&opts, 'r') || flag(&opts, 'R');
    // The last of -E/-G wins, as GNU grep does.
    let extended = opts
        .iter()
        .rev()
        .find(|(k, _)| *k == 'E' || *k == 'G')
        .is_some_and(|(k, _)| *k == 'E');
    // Without -E a pattern is a *basic* regular expression: `a\+` repeats and `a+`
    // is a literal plus. Getting this backwards is the classic porting trap, so the
    // same translator sed uses does the work here.
    let expression = patterns
        .iter()
        // A pattern may itself hold newlines: `grep -e $'a\nb'` is two alternatives.
        .flat_map(|p| p.split('\n').map(str::to_string).collect::<Vec<_>>())
        .map(|p| {
            let body = if flag(&opts, 'F') {
                regex::escape(&p)
            } else {
                crate::sed::translate(&p, extended)?
            };
            Ok(if flag(&opts, 'x') {
                format!("^(?:{body})$")
            } else if flag(&opts, 'w') {
                format!("\\b(?:{body})\\b")
            } else {
                format!("(?:{body})")
            })
        })
        .collect::<Result<Vec<_>, Fail>>()?
        .join("|");
    let regex = regex::RegexBuilder::new(&expression)
        .case_insensitive(flag(&opts, 'i'))
        .build()
        .map_err(|e| Fail::usage(e.to_string()))?;
    // Sources are (label, text); with no operand the pipeline's stdin is the source.
    let mut sources: Vec<(String, String)> = Vec::new();
    for operand in &rest {
        let path = c.resolve(operand);
        let directory = c.vfs.lstat(&path).is_ok_and(|m| m.is_dir);
        if directory && !recursive {
            if !flag(&opts, 's') {
                return Err(Fail::op("grep", operand, "Is a directory"));
            }
            continue;
        }
        let members: Vec<(String, String)> = if directory {
            walk_tree(c, &path)
                .into_iter()
                .filter(|(_, _, dir)| !dir)
                .map(|(p, _, _)| (p.clone(), p))
                .collect()
        } else {
            vec![(operand.clone(), path)]
        };
        for (label, file) in members {
            match c.vfs.read_as(&file, &c.user) {
                Ok(bytes) => sources.push((label, String::from_utf8_lossy(&bytes).into_owned())),
                Err(e) if flag(&opts, 's') => {
                    let _ = e;
                }
                Err(e) => return Err(Fail::io("grep", &label, &e)),
            }
        }
    }
    if rest.is_empty() {
        sources.push((String::from("(standard input)"), input.to_string()));
    }
    let labelled = if flag(&opts, 'h') {
        false
    } else {
        flag(&opts, 'H') || recursive || sources.len() > 1
    };
    let mut out = String::new();
    let mut matched = false;
    for (label, text) in &sources {
        let hits: Vec<(usize, &str)> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| regex.is_match(line) != flag(&opts, 'v'))
            .collect();
        matched |= !hits.is_empty();
        let prefix = if labelled {
            format!("{label}:")
        } else {
            String::new()
        };
        if flag(&opts, 'q') {
            continue;
        }
        if flag(&opts, 'c') {
            out.push_str(&format!("{prefix}{}\n", hits.len()));
        } else if flag(&opts, 'l') {
            if !hits.is_empty() {
                out.push_str(&format!("{label}\n"));
            }
        } else if flag(&opts, 'L') {
            if hits.is_empty() {
                out.push_str(&format!("{label}\n"));
            }
        } else if flag(&opts, 'o') {
            // Only the matched substrings; an inverted line has none to show.
            for (n, line) in hits {
                for found in regex.find_iter(line) {
                    if flag(&opts, 'n') {
                        out.push_str(&format!("{prefix}{}:{}\n", n + 1, found.as_str()));
                    } else {
                        out.push_str(&format!("{prefix}{}\n", found.as_str()));
                    }
                }
            }
        } else if context {
            // Context lines carry `-` where a matching line carries `:`, and a gap
            // between groups prints the `--` separator, exactly as GNU grep does.
            let all: Vec<&str> = text.lines().collect();
            let matches: std::collections::BTreeSet<usize> = hits.iter().map(|(n, _)| *n).collect();
            let mut shown = std::collections::BTreeSet::new();
            for n in &matches {
                for k in n.saturating_sub(before)..=(n + after).min(all.len().saturating_sub(1)) {
                    shown.insert(k);
                }
            }
            let mut previous: Option<usize> = None;
            for n in shown {
                if previous.is_some_and(|p| n > p + 1) {
                    out.push_str("--\n");
                }
                let mark = if matches.contains(&n) { ':' } else { '-' };
                let head = if labelled {
                    format!("{label}{mark}")
                } else {
                    String::new()
                };
                if flag(&opts, 'n') {
                    out.push_str(&format!("{head}{}{mark}{}\n", n + 1, all[n]));
                } else {
                    out.push_str(&format!("{head}{}\n", all[n]));
                }
                previous = Some(n);
            }
        } else {
            for (n, line) in hits {
                if flag(&opts, 'n') {
                    out.push_str(&format!("{prefix}{}:{line}\n", n + 1));
                } else {
                    out.push_str(&format!("{prefix}{line}\n"));
                }
            }
        }
    }
    // Status is the answer: 0 found, 1 not found. -c and -L still report it.
    let found = if flag(&opts, 'L') {
        !out.is_empty()
    } else {
        matched
    };
    if found {
        Ok(out)
    } else {
        Err(Fail::new(String::new(), 1).with_output(out))
    }
}

/// One `[ugoa][-+=][rwxXst]` clause. `conditional` is `X`: execute only where the node
/// is a directory or already carries an execute bit.
pub(crate) struct ModeClause {
    who: &'static str,
    op: char,
    perms: String,
}
/// A chmod mode: an octal literal, or symbolic clauses applied left to right.
pub(crate) enum ModeSpec {
    Absolute(u16),
    Symbolic(Vec<ModeClause>),
}
impl ModeSpec {
    pub(crate) fn parse(spec: &str) -> Result<Self, Fail> {
        if spec.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let mode = u16::from_str_radix(spec, 8)
                .ok()
                .filter(|m| *m <= 0o7777)
                .ok_or_else(|| Fail::usage(format!("chmod: invalid octal mode `{spec}`")))?;
            return Ok(ModeSpec::Absolute(mode));
        }
        let mut clauses = Vec::new();
        for part in spec.split(',') {
            let chars: Vec<char> = part.chars().collect();
            let mut i = 0;
            while chars.get(i).is_some_and(|c| "ugoa".contains(*c)) {
                i += 1;
            }
            let who: String = chars[..i].iter().collect();
            // No umask is modelled, so a bare `+x` means `a+x`.
            let who: &'static str = if who.is_empty() || who.contains('a') {
                "ugo"
            } else if who == "u" {
                "u"
            } else if who == "g" {
                "g"
            } else if who == "o" {
                "o"
            } else if who == "ug" || who == "gu" {
                "ug"
            } else if who == "uo" || who == "ou" {
                "uo"
            } else {
                "go"
            };
            if i == chars.len() {
                return Err(Fail::usage(format!("chmod: invalid mode `{spec}`")));
            }
            while i < chars.len() {
                let op = chars[i];
                if !"+-=".contains(op) {
                    return Err(Fail::usage(format!("chmod: invalid mode `{spec}`")));
                }
                i += 1;
                let start = i;
                while chars.get(i).is_some_and(|c| "rwxXst".contains(*c)) {
                    i += 1;
                }
                if chars.get(i).is_some_and(|c| "ugo".contains(*c)) {
                    return Err(Fail::usage(format!(
                        "chmod: copying permissions (`{spec}`) is not implemented"
                    )));
                }
                clauses.push(ModeClause {
                    who,
                    op,
                    perms: chars[start..i].iter().collect(),
                });
            }
        }
        Ok(ModeSpec::Symbolic(clauses))
    }
    pub(crate) fn apply(&self, mode: u16, is_dir: bool) -> u16 {
        let clauses = match self {
            ModeSpec::Absolute(m) => return *m,
            ModeSpec::Symbolic(v) => v,
        };
        let mut mode = mode & 0o7777;
        for clause in clauses {
            // `X` reads the mode as it stands after the clauses before it.
            let executable = clause.perms.contains('x')
                || (clause.perms.contains('X') && (is_dir || mode & 0o111 != 0));
            let (mut value, mut mask) = (0u16, 0u16);
            for (who, shift, special) in [('u', 6, 0o4000), ('g', 3, 0o2000), ('o', 0, 0o1000)] {
                if !clause.who.contains(who) {
                    continue;
                }
                mask |= (7 << shift) | special;
                if clause.perms.contains('r') {
                    value |= 4 << shift;
                }
                if clause.perms.contains('w') {
                    value |= 2 << shift;
                }
                if executable {
                    value |= 1 << shift;
                }
                if clause.perms.contains('s') && special != 0o1000 {
                    value |= special;
                }
                if clause.perms.contains('t') && special == 0o1000 {
                    value |= special;
                }
            }
            mode = match clause.op {
                '+' => mode | value,
                '-' => mode & !value,
                _ => (mode & !mask) | value,
            };
        }
        mode
    }
}
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_chmod(c: &mut Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options(
        "chmod",
        args,
        "Rv",
        "",
        &[("recursive", 'R'), ("verbose", 'v')],
    )?;
    let Some((spec, paths)) = rest.split_first() else {
        return Err(Fail::usage("chmod: missing operand"));
    };
    if paths.is_empty() {
        return Err(Fail::usage("chmod: missing operand"));
    }
    let change = ModeSpec::parse(spec)?;
    let mut told = String::new();
    for operand in paths {
        let root = c.resolve(operand);
        let meta = c
            .vfs
            .lstat(&root)
            .map_err(|_| format!("cannot access '{operand}': No such file or directory"))?;
        let targets: Vec<(String, bool)> = if flag(&opts, 'R') && meta.is_dir {
            walk_tree(c, &root)
                .into_iter()
                .map(|(path, _, dir)| (path, dir))
                .collect()
        } else {
            vec![(root, meta.is_dir)]
        };
        for (path, is_dir) in targets {
            let current = c.vfs.lstat(&path).map_err(Fail::from)?.mode;
            let wanted = change.apply(current, is_dir);
            c.vfs.chmod_as(&path, wanted, &c.user).map_err(Fail::from)?;
            if flag(&opts, 'v') {
                told.push_str(&format!(
                    "mode of '{path}' changed from {current:04o} to {wanted:04o}\n"
                ));
            }
        }
    }
    Ok(told)
}
/// Inverse of `clock`: a civil UTC date to a simulated tick. Times before the world's
/// epoch cannot be represented, so they are refused rather than clamped.
pub(crate) fn tick_from_civil(
    y: i64,
    m: i64,
    d: i64,
    hh: u64,
    mi: u64,
    ss: u64,
) -> Result<u64, Fail> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mi > 59 || ss > 60 {
        return Err(Fail::usage("touch: timestamp is not a valid date"));
    }
    let year = y - i64::from(m <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let unix = days * 86_400 + (hh * 3600 + mi * 60 + ss) as i64;
    let offset = unix - EPOCH_UNIX_SECONDS as i64;
    if offset < 0 {
        return Err(Fail::usage(
            "touch: timestamps before the simulated epoch (2026-09-17 09:00:00 UTC) \
             cannot be represented",
        ));
    }
    Ok(offset as u64 * 1_000_000)
}
/// `-t [[CC]YY]MMDDhhmm[.ss]`.
pub(crate) fn touch_stamp(value: &str) -> Result<u64, Fail> {
    let invalid = || Fail::usage(format!("touch: unsupported -t stamp `{value}`"));
    let (body, seconds) = match value.split_once('.') {
        Some((b, s)) => (b, s.parse::<u64>().map_err(|_| invalid())?),
        None => (value, 0),
    };
    if !body.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid());
    }
    let number = |from: usize, len: usize| -> Result<i64, Fail> {
        body.get(from..from + len)
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())
    };
    let (year, rest) = match body.len() {
        8 => (2026, 0),
        10 => (2000 + number(0, 2)?, 2),
        12 => (number(0, 4)?, 4),
        _ => return Err(invalid()),
    };
    tick_from_civil(
        year,
        number(rest, 2)?,
        number(rest + 2, 2)?,
        number(rest + 4, 2)? as u64,
        number(rest + 6, 2)? as u64,
        seconds,
    )
}
/// `-d`: `@SECONDS` or `YYYY-MM-DD[ |T]HH:MM[:SS]`. Relative words such as `yesterday`
/// are refused: there is no host clock to resolve them against.
pub(crate) fn touch_date(value: &str) -> Result<u64, Fail> {
    let invalid = || {
        Fail::usage(format!(
            "touch: unsupported -d date `{value}`; use @SECONDS or YYYY-MM-DD[ HH:MM[:SS]]"
        ))
    };
    if let Some(seconds) = value.strip_prefix('@') {
        let unix: i64 = seconds.parse().map_err(|_| invalid())?;
        return tick_from_civil(1970, 1, 1, 0, 0, 0).and_then(|_| {
            u64::try_from(unix - EPOCH_UNIX_SECONDS as i64)
                .map(|v| v * 1_000_000)
                .map_err(|_| {
                    Fail::usage(
                        "touch: timestamps before the simulated epoch cannot be represented",
                    )
                })
        });
    }
    let (date, time) = match value.split_once(['T', ' ']) {
        Some((d, t)) => (d, t),
        None => (value, "00:00:00"),
    };
    let day: Vec<&str> = date.split('-').collect();
    let clock: Vec<&str> = time.trim_end_matches('Z').split(':').collect();
    if day.len() != 3 || clock.len() < 2 || clock.len() > 3 {
        return Err(invalid());
    }
    let n = |v: &str| v.parse::<i64>().map_err(|_| invalid());
    tick_from_civil(
        n(day[0])?,
        n(day[1])?,
        n(day[2])?,
        n(clock[0])? as u64,
        n(clock[1])? as u64,
        clock.get(2).map(|v| n(v)).transpose()?.unwrap_or(0) as u64,
    )
}
/// The process table's columns, as `ps -o` names them. Each is read from the table:
/// nothing here is sampled from a host, and nothing is invented. `%CPU` and `TIME` are
/// a real counter that this world never charges (see `Process::cpu_us`), so they are
/// `0.0` and `00:00:00`; `RSS`, `VSZ` and `%MEM` come from the published footprint
/// model in `crates/machines/computer/src/process.rs`. Documented in `docs/shell.md`.
const PS_COLUMNS: &[(&str, &str, bool, usize)] = &[
    // (name, header, right-aligned, minimum width)
    ("pid", "PID", true, 7),
    ("ppid", "PPID", true, 7),
    ("pgid", "PGID", true, 7),
    ("user", "USER", false, 8),
    ("uid", "UID", false, 8),
    ("comm", "COMMAND", false, 0),
    ("args", "COMMAND", false, 0),
    ("stat", "STAT", false, 4),
    ("state", "S", false, 1),
    ("tty", "TTY", false, 8),
    ("time", "TIME", true, 8),
    ("etime", "ELAPSED", true, 11),
    ("etimes", "ELAPSED", true, 7),
    ("rss", "RSS", true, 6),
    ("vsz", "VSZ", true, 7),
    ("pmem", "%MEM", true, 4),
    ("pcpu", "%CPU", true, 4),
    // `C`, System V's CPU utilisation. No scheduler is simulated, so it is always 0.
    ("c", "C", true, 2),
    ("start", "START", true, 5),
];
/// `ps -o` aliases that name the same column under a different spelling.
pub(super) fn ps_canonical(field: &str) -> &str {
    match field {
        "ucomm" => "comm",
        "cmd" | "command" => "args",
        "s" => "state",
        "ruser" => "user",
        "pgrp" => "pgid",
        "tt" | "tname" => "tty",
        "cputime" => "time",
        "rsz" | "rssize" => "rss",
        "vsize" => "vsz",
        "%mem" => "pmem",
        "%cpu" => "pcpu",
        "stime" | "lstart" | "bsdstart" => "start",
        other => other,
    }
}
pub(super) fn ps_header(field: &str) -> Option<&'static str> {
    PS_COLUMNS
        .iter()
        .find(|(name, _, _, _)| *name == field)
        .map(|(_, header, _, _)| *header)
}
pub(super) fn ps_right(field: &str) -> bool {
    PS_COLUMNS
        .iter()
        .find(|(name, _, _, _)| *name == field)
        .is_some_and(|(_, _, right, _)| *right)
}
pub(super) fn ps_min_width(field: &str) -> usize {
    PS_COLUMNS
        .iter()
        .find(|(name, _, _, _)| *name == field)
        .map_or(0, |(_, _, _, width)| *width)
}
pub(super) fn ps_column_names() -> String {
    PS_COLUMNS
        .iter()
        .map(|(name, _, _, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}
/// Program name without its arguments or directory, the `comm`/`COMMAND` column.
pub(super) fn ps_program(command: &str) -> &str {
    command
        .split_whitespace()
        .find(|word| !word.contains('='))
        .unwrap_or_default()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
}
/// `[D-]HH:MM:SS` elapsed, the way `ps` prints `etime`.
pub(super) fn ps_elapsed(seconds: u64) -> String {
    let (days, hours, minutes, secs) = (
        seconds / 86_400,
        seconds % 86_400 / 3600,
        seconds % 3600 / 60,
        seconds % 60,
    );
    if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{secs:02}")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}
pub(super) fn ps_cell(field: &str, p: &crate::Process, c: &Computer, now: u64) -> String {
    let elapsed = now.saturating_sub(p.started) / 1_000_000;
    let defunct = matches!(p.state, crate::ProcessState::Zombie { .. });
    match field {
        "pid" => p.pid.to_string(),
        "ppid" => p.parent.to_string(),
        "pgid" => p.group.to_string(),
        "user" => p.owner.clone(),
        "uid" => {
            if p.owner == "root" {
                "0".into()
            } else {
                c.hardware.uid.to_string()
            }
        }
        "comm" => ps_program(&p.command).to_owned(),
        "args" => {
            if defunct {
                format!("{} <defunct>", p.command)
            } else {
                p.command.clone()
            }
        }
        "stat" | "state" => p.stat_letter().to_string(),
        "tty" => p.tty_name().to_owned(),
        "time" => {
            let s = p.cpu_us / 1_000_000;
            format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
        }
        "etime" => ps_elapsed(elapsed),
        "etimes" => elapsed.to_string(),
        "rss" => (p.rss_bytes / 1024).to_string(),
        "vsz" => (p.vsz_bytes() / 1024).to_string(),
        "pmem" => format!(
            "{:.1}",
            p.rss_bytes as f64 * 100.0 / c.hardware.memory_bytes.max(1) as f64
        ),
        "pcpu" => "0.0".into(),
        "c" => "0".into(),
        "start" => {
            let s = clock(p.started);
            format!("{:02}:{:02}", s.hour, s.minute)
        }
        _ => String::new(),
    }
}
/// The signed key `--sort` orders by. Numeric columns sort numerically; the rest sort
/// by their printed text, which is what `ps` does.
pub(super) fn ps_key(field: &str, p: &crate::Process, c: &Computer, now: u64) -> (i128, String) {
    let numeric = matches!(
        field,
        "pid" | "ppid" | "pgid" | "rss" | "vsz" | "etimes" | "time" | "pcpu" | "uid"
    );
    if numeric {
        let value = match field {
            "pid" => p.pid as i128,
            "ppid" => p.parent as i128,
            "pgid" => p.group as i128,
            "rss" => p.rss_bytes as i128,
            "vsz" => p.vsz_bytes() as i128,
            "etimes" => now.saturating_sub(p.started) as i128,
            "time" => p.cpu_us as i128,
            _ => 0,
        };
        return (value, String::new());
    }
    if field == "pmem" {
        return (p.rss_bytes as i128, String::new());
    }
    (0, ps_cell(field, p, c, now))
}
/// The processes a `ps` invocation selects, already sorted.
pub(super) struct PsSelection {
    rows: Vec<crate::Process>,
}
pub(super) fn ps_rows(
    c: &Computer,
    now: u64,
    everyone: bool,
    pids: &[u64],
    owners: &[String],
    sort: &[(bool, String)],
) -> PsSelection {
    let mut rows: Vec<crate::Process> = c
        .processes
        .list()
        .into_iter()
        .filter(|p| pids.is_empty() || pids.contains(&p.pid))
        .filter(|p| owners.is_empty() || owners.contains(&p.owner))
        .filter(|p| everyone || !pids.is_empty() || !owners.is_empty() || p.owner == c.user)
        .collect();
    if !sort.is_empty() {
        rows.sort_by(|a, b| {
            for (descending, field) in sort {
                let order = ps_key(field, a, c, now).cmp(&ps_key(field, b, c, now));
                let order = if *descending { order.reverse() } else { order };
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
            a.pid.cmp(&b.pid)
        });
    }
    PsSelection { rows }
}
/// Lay out columns the way `ps` does: every column but the last is padded to the
/// widest cell in it, and the last runs to the end of the line.
pub(super) fn ps_table(fields: &[String], headers: &[String], cells: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..fields.len())
        .map(|i| {
            cells
                .iter()
                .map(|row| row[i].chars().count())
                .chain(std::iter::once(headers[i].chars().count()))
                .chain(std::iter::once(ps_min_width(&fields[i])))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in std::iter::once(headers).chain(cells.iter().map(Vec::as_slice)) {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            if i + 1 == fields.len() && !ps_right(&fields[i]) {
                line.push_str(cell);
            } else if ps_right(&fields[i]) {
                line.push_str(&" ".repeat(widths[i].saturating_sub(cell.chars().count())));
                line.push_str(cell);
            } else {
                line.push_str(cell);
                line.push_str(&" ".repeat(widths[i].saturating_sub(cell.chars().count())));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
/// `ps`, `ps aux`, `ps -ef`, `ps -e -o pid,rss,comm --sort=-rss`. Everything printed is
/// read from the process table; see *Process table* in `docs/shell.md` for the column
/// schema and for what this world does and does not account for.
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_ps(c: &Computer, args: &[String], t: u64) -> Result<String, Fail> {
    // BSD syntax: a leading option word with no dash, as in `ps aux`.
    let mut args = args.to_vec();
    let mut bsd = String::new();
    if args
        .first()
        .is_some_and(|a| !a.is_empty() && a.chars().all(|ch| "auxewfjr".contains(ch)))
    {
        bsd = args.remove(0);
    }
    let (opts, rest) = options(
        "ps",
        &args,
        "efAwjr",
        "upos",
        &[
            ("json", 'j'),
            ("full", 'f'),
            ("user", 'u'),
            ("pid", 'p'),
            ("every", 'e'),
            ("format", 'o'),
            ("sort", 's'),
        ],
    )?;
    if let Some(operand) = rest.first() {
        return Err(Fail::usage(format!(
            "ps: unsupported operand `{operand}`; this world models `ps`, `ps aux`, \
             `ps -ef`, `ps -e`/`-A`, `-f`, `-u USER`, `-p PID`, `-o COLUMNS`, \
             `--sort=[+-]COLUMN` and `--json`"
        )));
    }
    let mut pids = Vec::new();
    for value in opts.iter().filter(|(k, _)| *k == 'p').map(|(_, v)| v) {
        for part in value.split(',').filter(|s| !s.is_empty()) {
            pids.push(
                part.parse::<u64>()
                    .map_err(|_| Fail::usage("ps: option `-p` expects a pid"))?,
            );
        }
    }
    let owners: Vec<String> = opts
        .iter()
        .filter(|(k, _)| *k == 'u')
        .flat_map(|(_, v)| v.split(',').map(str::to_owned))
        .filter(|s| !s.is_empty())
        .collect();
    // `ps aux`'s `u` is a format, not a selector; `-u USER` is a selector.
    let bsd_user = bsd.contains('u');
    let mut sort: Vec<(bool, String)> = Vec::new();
    for spec in opts.iter().filter(|(k, _)| *k == 's').map(|(_, v)| v) {
        for key in spec.split(',').filter(|s| !s.is_empty()) {
            let (descending, name) = match key.as_bytes()[0] {
                b'-' => (true, &key[1..]),
                b'+' => (false, &key[1..]),
                _ => (false, key),
            };
            let name = ps_canonical(name);
            if ps_header(name).is_none() {
                return Err(Fail::usage(format!(
                    "ps: unknown sort column `{name}`; this world sorts by {}",
                    ps_column_names()
                )));
            }
            sort.push((descending, name.to_owned()));
        }
    }
    let selected: Vec<String> = match value(&opts, 'o') {
        Some(spec) => {
            let mut fields = Vec::new();
            for part in spec.split(',').filter(|s| !s.is_empty()) {
                // `-o rss=RSS` renames a column; this world takes the column and
                // refuses the rename rather than printing a header it did not pick.
                if part.contains('=') {
                    return Err(Fail::usage(
                        "ps: `-o` header renaming is not modelled; name the column alone",
                    ));
                }
                let name = ps_canonical(part);
                if ps_header(name).is_none() {
                    return Err(Fail::usage(format!(
                        "ps: unsupported column `{part}`; this world models {}",
                        ps_column_names()
                    )));
                }
                fields.push(name.to_owned());
            }
            if fields.is_empty() {
                return Err(Fail::usage("ps: option `-o` expects a column list"));
            }
            fields
        }
        None if bsd_user => [
            "user", "pid", "pcpu", "pmem", "vsz", "rss", "tty", "stat", "start", "time", "args",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect(),
        None if flag(&opts, 'f') => ["user", "pid", "ppid", "c", "start", "tty", "time", "args"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        None => ["pid", "tty", "time", "args"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
    };
    let everyone = flag(&opts, 'e')
        || flag(&opts, 'A')
        || flag(&opts, 'j')
        || bsd.contains('a')
        || bsd.contains('x');
    let selection = ps_rows(c, t, everyone, &pids, &owners, &sort);
    if flag(&opts, 'j') {
        return Ok(serde_json::to_string_pretty(&selection.rows)
            .map(|s| s + "\n")
            .map_err(|e| e.to_string())?);
    }
    // System V formats head the command column `CMD`; BSD and `-o` head it `COMMAND`.
    let short = value(&opts, 'o').is_none() && !bsd_user;
    let full = short && flag(&opts, 'f');
    let headers: Vec<String> = selected
        .iter()
        .map(|f| match f.as_str() {
            "args" if short => "CMD".to_owned(),
            // `-f` heads the start column STIME and the owner column UID, as System V
            // does; it prints the owner's name there, as `ps -ef` does.
            "start" if full => "STIME".to_owned(),
            "user" if full => "UID".to_owned(),
            _ => ps_header(f).unwrap_or("").to_owned(),
        })
        .collect();
    let cells: Vec<Vec<String>> = selection
        .rows
        .iter()
        .map(|p| selected.iter().map(|f| ps_cell(f, p, c, t)).collect())
        .collect();
    Ok(ps_table(&selected, &headers, &cells))
}
/// `top -b -n1`: one batch snapshot of the same table `ps` reads. An interactive `top`
/// would need a terminal this world does not model, so only the batch form exists.
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_top(c: &Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, rest) = options(
        "top",
        args,
        "b",
        "no",
        &[
            ("batch-mode", 'b'),
            ("iterations", 'n'),
            ("sort-override", 'o'),
        ],
    )?;
    if let Some(operand) = rest.first() {
        return Err(Fail::usage(format!(
            "top: unsupported operand `{operand}`; this world models `top -b -n1`"
        )));
    }
    if !flag(&opts, 'b') {
        return Err(Fail::usage(
            "top: interactive mode needs a terminal this world does not model; use `top -b -n1`",
        ));
    }
    match value(&opts, 'n') {
        Some("1") => {}
        Some(other) => {
            return Err(Fail::usage(format!(
                "top: `-n {other}` would need simulated time to pass between samples; \
                 use `top -b -n1` and advance the clock between snapshots"
            )))
        }
        None => {
            return Err(Fail::usage(
                "top: `-b` needs an iteration count; use `top -b -n1`",
            ))
        }
    }
    let sort = match value(&opts, 'o') {
        Some(spec) => {
            let (descending, name) = match spec.as_bytes().first() {
                Some(b'-') => (true, &spec[1..]),
                Some(b'+') => (false, &spec[1..]),
                _ => (true, spec),
            };
            let lowered = name.to_lowercase();
            let name = ps_canonical(&lowered);
            if ps_header(name).is_none() {
                return Err(Fail::usage(format!("top: unknown sort column `{spec}`")));
            }
            vec![(descending, name.to_owned())]
        }
        None => vec![(true, "rss".to_owned())],
    };
    let rows = ps_rows(c, t, true, &[], &[], &sort).rows;
    let total = c.hardware.memory_bytes;
    let used: u64 = rows.iter().map(|p| p.rss_bytes).sum();
    let mib = |bytes: u64| bytes as f64 / (1 << 20) as f64;
    let counted = |want: char| {
        rows.iter()
            .filter(|p| p.stat_letter() == want)
            .count()
            .to_string()
    };
    let now = clock(t);
    let up = t.saturating_sub(c.hardware.boot_tick) / 1_000_000;
    let mut out = format!(
        "top - {:02}:{:02}:{:02} up {},  1 user,  load average: 0.00, 0.00, 0.00\n",
        now.hour,
        now.minute,
        now.second,
        if up >= 3600 {
            format!("{:2}:{:02}", up / 3600, up % 3600 / 60)
        } else {
            format!("{} min", up / 60)
        }
    );
    out.push_str(&format!(
        "Tasks: {:>4} total, {:>4} running, {:>4} sleeping, {:>4} stopped, {:>4} zombie\n",
        rows.len(),
        counted('R'),
        counted('S'),
        counted('T'),
        counted('Z'),
    ));
    // No scheduler is simulated, so the only honest CPU line is an idle one.
    out.push_str(
        "%Cpu(s):  0.0 us,  0.0 sy,  0.0 ni,100.0 id,  0.0 wa,  0.0 hi,  0.0 si,  0.0 st\n",
    );
    out.push_str(&format!(
        "MiB Mem : {:9.1} total, {:9.1} free, {:9.1} used, {:9.1} buff/cache\n",
        mib(total),
        mib(total.saturating_sub(used)),
        mib(used),
        0.0
    ));
    out.push_str(&format!(
        "MiB Swap: {:9.1} total, {:9.1} free, {:9.1} used. {:9.1} avail Mem\n\n",
        0.0,
        0.0,
        0.0,
        mib(total.saturating_sub(used))
    ));
    // PR and NI are fixed: this world models no scheduler and refuses `nice`.
    out.push_str("    PID USER      PR  NI    VIRT    RES  S  %CPU  %MEM     TIME+ COMMAND\n");
    for p in &rows {
        out.push_str(&format!(
            "{:>7} {:<9} 20   0 {:>7} {:>6}  {} {:>5} {:>5} {:>9} {}\n",
            p.pid,
            p.owner,
            p.vsz_bytes() / 1024,
            p.rss_bytes / 1024,
            p.stat_letter(),
            "0.0",
            format!(
                "{:.1}",
                p.rss_bytes as f64 * 100.0 / c.hardware.memory_bytes.max(1) as f64
            ),
            format!(
                "{}:{:02}.{:02}",
                p.cpu_us / 60_000_000,
                p.cpu_us % 60_000_000 / 1_000_000,
                p.cpu_us % 1_000_000 / 10_000
            ),
            ps_program(&p.command),
        ));
    }
    Ok(out)
}
/// The processes a `pgrep`/`pkill` pattern selected, and the options it was given.
type Matched = (Vec<crate::Process>, Vec<(char, String)>);
/// The processes `pgrep`/`pkill` select. Matching is a plain substring of the program
/// name, or of the whole command line with `-f`; this world models no regular
/// expressions here and says so rather than matching one badly.
pub(super) fn pgrep_select(name: &str, c: &Computer, args: &[String]) -> Result<Matched, Fail> {
    let (opts, rest) = options(
        name,
        args,
        "flxn",
        "u",
        &[
            ("full", 'f'),
            ("list-name", 'l'),
            ("exact", 'x'),
            ("newest", 'n'),
            ("euid", 'u'),
        ],
    )?;
    let pattern = match rest.as_slice() {
        [one] => one.clone(),
        [] if value(&opts, 'u').is_some() => String::new(),
        [] => return Err(Fail::usage(format!("{name}: missing pattern"))),
        _ => {
            return Err(Fail::usage(format!(
                "{name}: expected one pattern; this world matches a substring, not a regex"
            )))
        }
    };
    if pattern.contains(['*', '?', '[', '^', '$', '|', '+']) {
        return Err(Fail::usage(format!(
            "{name}: regular expressions are not modelled; the pattern is matched as a \
             plain substring of the program name, or of the command line with `-f`"
        )));
    }
    let owners: Vec<String> = value(&opts, 'u')
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let full = flag(&opts, 'f');
    let exact = flag(&opts, 'x');
    // `pgrep` never reports itself. The running command is the newest process, the
    // same convention the interpreters use to name the process they run as.
    let own = c.processes.list().iter().map(|p| p.pid).max().unwrap_or(0);
    let mut rows: Vec<crate::Process> = c
        .processes
        .list()
        .into_iter()
        .filter(|p| p.pid != 1 && p.pid != own)
        .filter(|p| owners.is_empty() || owners.contains(&p.owner))
        .filter(|p| {
            let subject = if full {
                p.command.as_str()
            } else {
                ps_program(&p.command)
            };
            if pattern.is_empty() {
                true
            } else if exact {
                subject == pattern
            } else {
                subject.contains(&pattern)
            }
        })
        .collect();
    rows.sort_by_key(|p| p.pid);
    if flag(&opts, 'n') {
        rows = rows.into_iter().next_back().into_iter().collect();
    }
    Ok((rows, opts))
}
/// `free`. Total memory is the machine's fixed `hardware.memory_bytes`; used is the sum
/// of what the running processes are modelled to hold. No buffers, cache or swap are
/// simulated, so those columns are zero rather than invented.
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_free(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options(
        "free",
        args,
        "bkmgh",
        "",
        &[
            ("bytes", 'b'),
            ("kibi", 'k'),
            ("mebi", 'm'),
            ("gibi", 'g'),
            ("human", 'h'),
        ],
    )?;
    if let Some(operand) = rest.first() {
        return Err(Fail::usage(format!(
            "free: unsupported operand `{operand}`"
        )));
    }
    let total = c.hardware.memory_bytes;
    let used: u64 = c.processes.list().iter().map(|p| p.rss_bytes).sum();
    let used = used.min(total);
    let free = total - used;
    let show = |bytes: u64| -> String {
        if flag(&opts, 'h') {
            let text = human(bytes);
            if text.chars().last().is_some_and(|ch| ch.is_ascii_digit()) {
                format!("{text}B")
            } else {
                format!("{text}i")
            }
        } else if flag(&opts, 'b') {
            bytes.to_string()
        } else if flag(&opts, 'm') {
            (bytes >> 20).to_string()
        } else if flag(&opts, 'g') {
            (bytes >> 30).to_string()
        } else {
            (bytes >> 10).to_string()
        }
    };
    let mut out = format!(
        "{:<15}{:>12}{:>12}{:>12}{:>12}{:>12}{:>12}\n",
        "", "total", "used", "free", "shared", "buff/cache", "available"
    );
    out.push_str(&format!(
        "{:<15}{:>12}{:>12}{:>12}{:>12}{:>12}{:>12}\n",
        "Mem:",
        show(total),
        show(used),
        show(free),
        show(0),
        show(0),
        show(free)
    ));
    out.push_str(&format!(
        "{:<15}{:>12}{:>12}{:>12}\n",
        "Swap:",
        show(0),
        show(0),
        show(0)
    ));
    Ok(out)
}
/// `lsof`-lite: the file descriptors and listening sockets the process table really
/// holds. `DEVICE` and `SIZE/OFF` are not printed because this world models neither a
/// device table nor a shared offset; everything shown is read from the table.
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_lsof(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options("lsof", args, "n", "pu", &[("pid", 'p'), ("user", 'u')])?;
    let mut pids = Vec::new();
    for spec in opts.iter().filter(|(k, _)| *k == 'p').map(|(_, v)| v) {
        for part in spec.split(',').filter(|s| !s.is_empty()) {
            pids.push(
                part.parse::<u64>()
                    .map_err(|_| Fail::usage("lsof: option `-p` expects a pid"))?,
            );
        }
    }
    let owners: Vec<&str> = opts
        .iter()
        .filter(|(k, _)| *k == 'u')
        .flat_map(|(_, v)| v.split(','))
        .filter(|s| !s.is_empty())
        .collect();
    let want = rest.first().map(|path| c.resolve(path));
    let mut out = format!(
        "{:<12} {:>7} {:<10} {:<5} {:<6} {:>10} {}\n",
        "COMMAND", "PID", "USER", "FD", "TYPE", "NODE", "NAME"
    );
    let mut matched = false;
    for p in c.processes.list() {
        if !pids.is_empty() && !pids.contains(&p.pid) {
            continue;
        }
        if !owners.is_empty() && !owners.iter().any(|o| *o == p.owner) {
            continue;
        }
        let mut rows: Vec<(String, &'static str, String, String)> = Vec::new();
        for (fd, descriptor) in &p.fds {
            let (kind, name, node) = match descriptor {
                crate::FileDescriptor::Stdin
                | crate::FileDescriptor::Stdout
                | crate::FileDescriptor::Stderr => {
                    ("CHR", format!("/dev/{}", p.tty_name()), String::new())
                }
                crate::FileDescriptor::File { path, .. } => (
                    if c.vfs.stat(path).is_ok_and(|m| m.is_dir) {
                        "DIR"
                    } else {
                        "REG"
                    },
                    path.clone(),
                    c.vfs
                        .stat(path)
                        .map(|m| m.inode.to_string())
                        .unwrap_or_default(),
                ),
                crate::FileDescriptor::Pipe { pipe, write } => (
                    "FIFO",
                    format!("pipe:[{pipe}]{}", if *write { " (write)" } else { "" }),
                    pipe.to_string(),
                ),
                crate::FileDescriptor::Socket { listener } => {
                    ("IPv4", listener.clone(), String::new())
                }
            };
            let suffix = match descriptor {
                crate::FileDescriptor::Stdin => "r",
                crate::FileDescriptor::Stdout | crate::FileDescriptor::Stderr => "w",
                crate::FileDescriptor::File { writable: true, .. } => "u",
                crate::FileDescriptor::File { .. } => "r",
                crate::FileDescriptor::Pipe { write: true, .. } => "w",
                crate::FileDescriptor::Pipe { .. } => "r",
                crate::FileDescriptor::Socket { .. } => "u",
            };
            rows.push((format!("{fd}{suffix}"), kind, node, name));
        }
        for listener in &p.listeners {
            rows.push(("LISTEN".into(), "IPv4", String::new(), listener.clone()));
        }
        for (fd, kind, node, name) in rows {
            if want.as_deref().is_some_and(|w| w != name) {
                continue;
            }
            matched = true;
            out.push_str(&format!(
                "{:<12} {:>7} {:<10} {:<5} {:<6} {:>10} {}\n",
                ps_program(&p.command),
                p.pid,
                p.owner,
                fd,
                kind,
                node,
                name
            ));
        }
    }
    if !matched {
        // lsof's own convention: nothing open is status 1, with no rows printed.
        return Err(Fail::new(String::new(), 1));
    }
    Ok(out)
}
/// `apps`: the applications this machine has installed, the shell's view of the same
/// list `application.v1 list` returns. Ids only — a launcher label belongs to a desktop
/// shell, and this machine's own record is the set of ids.
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_apps(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options("apps", args, "j", "", &[("json", 'j')])?;
    if let Some(operand) = rest.first() {
        return Err(Fail::usage(format!(
            "apps: unsupported operand `{operand}`"
        )));
    }
    let ids: Vec<&str> = c.installed_apps.iter().map(String::as_str).collect();
    if flag(&opts, 'j') {
        return serde_json::to_string(&ids)
            .map(|s| s + "\n")
            .map_err(|e| Fail::from(e.to_string()));
    }
    Ok(ids.iter().map(|id| format!("{id}\n")).collect::<String>())
}
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_uptime(c: &Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, rest) = options("uptime", args, "ps", "", &[("pretty", 'p'), ("since", 's')])?;
    if !rest.is_empty() {
        return Err(Fail::usage("uptime: takes no operands"));
    }
    let boot = c.hardware.boot_tick;
    let elapsed = t.saturating_sub(boot) / 1_000_000;
    if flag(&opts, 's') {
        return Ok(format!("{}\n", clock(boot).stamp()));
    }
    let (days, hours, minutes) = (
        elapsed / 86_400,
        elapsed % 86_400 / 3600,
        elapsed % 3600 / 60,
    );
    let plural = |n: u64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    if flag(&opts, 'p') {
        let mut parts = Vec::new();
        if days > 0 {
            parts.push(plural(days, "day"));
        }
        if hours > 0 {
            parts.push(plural(hours, "hour"));
        }
        parts.push(plural(minutes, "minute"));
        return Ok(format!("up {}\n", parts.join(", ")));
    }
    let span = if days > 0 {
        format!("{} days, {hours:2}:{minutes:02}", days)
    } else if hours > 0 {
        format!("{hours:2}:{minutes:02}")
    } else {
        format!("{minutes} min")
    };
    let now = clock(t);
    Ok(format!(
        " {:02}:{:02}:{:02} up {span},  1 user,  load average: 0.00, 0.00, 0.00\n",
        now.hour, now.minute, now.second
    ))
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_which(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, names) = options("which", args, "a", "", &[("all", 'a')])?;
    if names.is_empty() {
        return Err(Fail::usage("which: missing operand"));
    }
    let separator = if c.dialect == "powershell" { ';' } else { ':' };
    let mut out = String::new();
    let mut missing = 0;
    for name in &names {
        let mut hits = Vec::new();
        for dir in c
            .env
            .get("PATH")
            .map(String::as_str)
            .unwrap_or("/bin:/usr/bin")
            .split(separator)
        {
            let candidate = c.resolve(&format!("{dir}/{name}"));
            if c.vfs.exists(&candidate) {
                hits.push(candidate);
            }
        }
        // Built-ins run in-process: report the conventional path so "is it
        // available?" gets a truthful yes even though no binary exists.
        if hits.is_empty() && BUILTINS.contains(&name.to_ascii_lowercase().as_str()) {
            hits.push(format!("/usr/bin/{name}"));
        }
        if hits.is_empty() {
            missing += 1;
        }
        for hit in hits
            .iter()
            .take(if flag(&opts, 'a') { usize::MAX } else { 1 })
        {
            out.push_str(hit);
            out.push('\n');
        }
    }
    if missing == names.len() {
        Err(Fail::new(String::new(), 1).with_output(out))
    } else {
        Ok(out)
    }
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_du(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, mut paths) = options(
        "du",
        args,
        "sahkbcm",
        "d",
        &[
            ("summarize", 's'),
            ("all", 'a'),
            ("human-readable", 'h'),
            ("bytes", 'b'),
            ("total", 'c'),
            ("max-depth", 'd'),
        ],
    )?;
    if paths.is_empty() {
        paths.push(".".into());
    }
    let limit = match value(&opts, 'd') {
        Some(v) => Some(
            v.parse::<usize>()
                .map_err(|_| Fail::usage("du: --max-depth expects a number"))?,
        ),
        None => None,
    };
    let render = |bytes: u64| {
        if flag(&opts, 'b') {
            bytes.to_string()
        } else if flag(&opts, 'h') {
            human(allocated(bytes))
        } else if flag(&opts, 'm') {
            (allocated(bytes) / (1 << 20)).to_string()
        } else {
            (allocated(bytes) / 1024).to_string()
        }
    };
    let mut out = String::new();
    let mut grand = 0u64;
    for operand in &paths {
        let root = c.resolve(operand);
        c.vfs
            .lstat(&root)
            .map_err(|_| format!("cannot access '{operand}': No such file or directory"))?;
        let nodes = walk_tree(c, &root);
        // Each node's bytes roll up into every ancestor inside the root.
        let mut totals: BTreeMap<String, u64> = BTreeMap::new();
        for (path, size, _) in &nodes {
            let mut current = path.clone();
            loop {
                *totals.entry(current.clone()).or_default() += allocated(*size);
                if current == root || current.len() <= root.len() {
                    break;
                }
                current = current
                    .rsplit_once('/')
                    .map(|(a, _)| a.to_string())
                    .unwrap_or_default();
                if current.is_empty() {
                    current = "/".into();
                }
            }
        }
        grand += totals.get(&root).copied().unwrap_or(0);
        let depth_of = |path: &str| path[root.len().min(path.len())..].matches('/').count();
        let mut rows: Vec<(String, u64)> = nodes
            .iter()
            .filter(|(path, _, dir)| {
                (*dir || flag(&opts, 'a'))
                    && !flag(&opts, 's')
                    && limit.is_none_or(|max| depth_of(path) <= max)
            })
            .map(|(path, _, _)| (path.clone(), totals.get(path).copied().unwrap_or(0)))
            .collect();
        // Children before their parent, as du walks depth-first.
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        rows.sort_by_key(|(path, _)| std::cmp::Reverse(depth_of(path)));
        if flag(&opts, 's') {
            rows.push((root.clone(), totals.get(&root).copied().unwrap_or(0)));
        }
        for (path, bytes) in rows {
            let label = if path == root {
                operand.clone()
            } else {
                format!("{}{}", operand.trim_end_matches('/'), &path[root.len()..])
            };
            out.push_str(&format!("{}\t{label}\n", render(bytes)));
        }
    }
    if flag(&opts, 'c') {
        out.push_str(&format!("{}\ttotal\n", render(grand)));
    }
    Ok(out)
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_df(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, paths) = options(
        "df",
        args,
        "hkT",
        "",
        &[("human-readable", 'h'), ("print-type", 'T')],
    )?;
    for operand in &paths {
        c.vfs
            .lstat(&c.resolve(operand))
            .map_err(|_| format!("{operand}: No such file or directory"))?;
    }
    // One filesystem: capacity is a fixed fact, usage is summed from the VFS.
    let total = c.hardware.disk_bytes;
    let used: u64 = walk_tree(c, "/")
        .iter()
        .map(|(_, size, _)| allocated(*size))
        .sum();
    let free = total.saturating_sub(used);
    let percent = if total == 0 {
        0
    } else {
        used.div_ceil(total.div_ceil(100)).min(100)
    };
    let cell = |bytes: u64| {
        if flag(&opts, 'h') {
            human(bytes)
        } else {
            (bytes / 1024).to_string()
        }
    };
    let kind = if flag(&opts, 'T') { "ext4     " } else { "" };
    let head = if flag(&opts, 'T') { "Type     " } else { "" };
    Ok(format!(
            "Filesystem     {head}{:>10} {:>10} {:>10} Use% Mounted on\n{:<14} {kind}{:>10} {:>10} {:>10} {:>3}% /\n",
            if flag(&opts, 'h') { "Size" } else { "1K-blocks" },
            "Used",
            if flag(&opts, 'h') { "Avail" } else { "Available" },
            c.hardware.device,
            cell(total),
            cell(used),
            cell(free),
            percent
        ))
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
pub(super) fn cmd_ip(c: &Computer, args: &[String]) -> Result<String, Fail> {
    // -4/-6/-o/-brief would filter or reshape the printout; this world's NIC summary
    // is a fixed block, so they are refused rather than accepted and dropped.
    let (_, rest) = options("ip", args, "", "", &[])?;
    let object = rest.first().map(String::as_str).unwrap_or("");
    let action = rest.get(1).map(String::as_str).unwrap_or("show");
    if !matches!(action, "show" | "list" | "s" | "l") {
        return Err(Fail::usage(format!(
            "ip: unsupported action `{action}`; the simulated NIC is read-only"
        )));
    }
    let h = &c.hardware;
    let octets: Vec<u32> = h.ipv4.split('.').filter_map(|p| p.parse().ok()).collect();
    let address = if octets.len() == 4 {
        octets.iter().fold(0u32, |acc, o| (acc << 8) | o)
    } else {
        0
    };
    let mask = if h.prefix >= 32 {
        u32::MAX
    } else {
        !(u32::MAX >> h.prefix)
    };
    let quad = |v: u32| format!("{}.{}.{}.{}", v >> 24, v >> 16 & 255, v >> 8 & 255, v & 255);
    let (network, broadcast) = (quad(address & mask), quad(address | !mask));
    match object {
            "a" | "addr" | "address" => Ok(format!(
                "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN group default qlen 1000\n    \
                 link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00\n    \
                 inet 127.0.0.1/8 scope host lo\n       valid_lft forever preferred_lft forever\n\
                 2: {nic}: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP group default qlen 1000\n    \
                 link/ether {mac} brd ff:ff:ff:ff:ff:ff\n    \
                 inet {ip}/{prefix} brd {broadcast} scope global {nic}\n       valid_lft forever preferred_lft forever\n",
                nic = h.interface, mac = h.mac, ip = h.ipv4, prefix = h.prefix,
            )),
            "l" | "link" => Ok(format!(
                "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\n    \
                 link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00\n\
                 2: {nic}: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP mode DEFAULT group default qlen 1000\n    \
                 link/ether {mac} brd ff:ff:ff:ff:ff:ff\n",
                nic = h.interface, mac = h.mac,
            )),
            "r" | "route" => Ok(format!(
                "default via {gw} dev {nic} proto dhcp metric 100\n\
                 {network}/{prefix} dev {nic} proto kernel scope link src {ip} metric 100\n",
                gw = h.gateway, nic = h.interface, prefix = h.prefix, ip = h.ipv4,
            )),
            "" => Err(Fail::usage("ip: missing object; try `ip addr`, `ip link` or `ip route`")),
            other => Err(Fail::usage(format!(
                "ip: unsupported object `{other}`; this world models addr, link and route only"
            ))),
        }
}
