//! `gzip`, `gunzip` and `zcat` over the VFS, with `cw-zlib`.
//!
//! Files are compressed in place (`a.txt` becomes `a.txt.gz` with GNU gzip's header:
//! the original name, the file's modification time, OS 3) and decompressed back.
//! Decompressed text may go to standard output (`zcat`, `-c` with `-d`); compressed
//! bytes cannot, because the shell's pipes carry text — `gzip -c` is refused.
use crate::{CommandResult, Computer};

const EPOCH: u64 = crate::shell::EPOCH_UNIX_SECONDS;

fn usage(name: &str, msg: &str) -> CommandResult {
    CommandResult::new(format!("{name}: {msg}\n"), 2)
}

/// The suffix `gunzip` strips, and what it leaves behind.
fn strip_gz_suffix(name: &str) -> Option<String> {
    for (suffix, replacement) in [
        (".tar.gz", ".tar"),
        (".tgz", ".tar"),
        (".taz", ".tar"),
        (".gz", ""),
        (".z", ""),
        (".Z", ""),
        ("-gz", ""),
        ("-z", ""),
    ] {
        if let Some(stem) = name.strip_suffix(suffix) {
            if !stem.is_empty() {
                return Some(format!("{stem}{replacement}"));
            }
        }
    }
    None
}

/// gzip keeps the file it replaced: the new name carries the old mode and times.
fn carry_attributes(c: &mut Computer, from: &str, to: &str, user: &str, tick: u64) {
    let Ok(meta) = c.vfs.lstat(from) else { return };
    let _ = c.vfs.chmod_as(to, meta.mode, user);
    let _ = c.vfs.set_times_as(
        to,
        Some(meta.accessed),
        Some(meta.modified),
        user,
        tick,
        true,
    );
}

pub fn execute(c: &mut Computer, args: &[String], input: &str, tick: u64) -> CommandResult {
    let name = args[0].rsplit('/').next().unwrap_or(&args[0]).to_string();
    let mut decompress = name == "gunzip" || name == "zcat";
    let mut stdout = name == "zcat";
    let mut keep = false;
    let mut force = false;
    let mut verbose = false;
    let mut list = false;
    let mut test = false;
    let mut level = 6;
    let mut files = vec![];
    let mut only_files = false;
    for a in &args[1..] {
        if only_files || !a.starts_with('-') || a == "-" {
            files.push(a.clone());
            continue;
        }
        if a == "--" {
            only_files = true;
            continue;
        }
        let long = match a.as_str() {
            "--decompress" | "--uncompress" => Some('d'),
            "--stdout" | "--to-stdout" => Some('c'),
            "--keep" => Some('k'),
            "--force" => Some('f'),
            "--verbose" => Some('v'),
            "--list" => Some('l'),
            "--test" => Some('t'),
            "--fast" => Some('1'),
            "--best" => Some('9'),
            "--quiet" => Some('q'),
            _ if a.starts_with("--") => {
                return usage(&name, &format!("unrecognized option '{a}'"));
            }
            _ => None,
        };
        let chars: Vec<char> = match long {
            Some(ch) => vec![ch],
            None => a[1..].chars().collect(),
        };
        for ch in chars {
            match ch {
                'd' => decompress = true,
                'c' => stdout = true,
                'k' => keep = true,
                'f' => force = true,
                'v' => verbose = true,
                'l' => list = true,
                't' => test = true,
                'q' | 'n' | 'N' => {}
                '1'..='9' => level = ch.to_digit(10).unwrap() as i32,
                other => return usage(&name, &format!("invalid option -- '{other}'")),
            }
        }
    }
    if files.is_empty() || files == ["-"] {
        if !decompress {
            return CommandResult::new(
                format!("{name}: compressed data not written to stdout. This shell's stdout is text, so name an output file instead.\n"),
                2,
            );
        }
        // Standard input carries text, so it cannot hold compressed data.
        let _ = input;
        return CommandResult::new(format!("{name}: stdin: not in gzip format\n"), 1);
    }
    if stdout && !decompress {
        return CommandResult::new(
            format!("{name}: compressed data not written to stdout. This shell's stdout is text, so name an output file instead.\n"),
            2,
        );
    }
    let user = c.user.clone();
    let mut out = String::new();
    let mut err = String::new();
    let mut status = 0;
    if list {
        out.push_str("         compressed        uncompressed  ratio uncompressed_name\n");
    }
    for f in files {
        let path = c.resolve(&f);
        let bytes = match c.vfs.read_as(&path, &user) {
            Ok(b) => b,
            Err(_) if c.vfs.stat(&path).is_ok_and(|m| m.is_dir) => {
                err.push_str(&format!("{name}: {f} is a directory -- ignored\n"));
                status = 2.max(status);
                continue;
            }
            Err(_) => {
                err.push_str(&format!("{name}: {f}: No such file or directory\n"));
                status = 1;
                continue;
            }
        };
        if decompress || list || test {
            if strip_gz_suffix(&f).is_none() && !stdout && !test && !list {
                err.push_str(&format!("{name}: {f}: unknown suffix -- ignored\n"));
                status = 2.max(status);
                continue;
            }
            let data = match cw_zlib::gunzip_members(&bytes) {
                Ok(d) => d,
                Err(e) => {
                    let why = match e {
                        cw_zlib::ZError::Buf => "unexpected end of file".to_string(),
                        cw_zlib::ZError::Data(m) if m.contains("header") => {
                            "not in gzip format".into()
                        }
                        other => format!("invalid compressed data--{}", other.message()),
                    };
                    err.push_str(&format!("{name}: {f}: {why}\n"));
                    status = 1;
                    continue;
                }
            };
            if list {
                let target = f.trim_end_matches(".gz");
                let ratio = if data.is_empty() {
                    0.0
                } else {
                    (1.0 - bytes.len() as f64 / data.len() as f64) * 100.0
                };
                out.push_str(&format!(
                    "{:>19} {:>19} {:>5.1}% {target}\n",
                    bytes.len(),
                    data.len(),
                    ratio
                ));
                continue;
            }
            if test {
                if verbose {
                    err.push_str(&format!("{f}:\t OK\n"));
                }
                continue;
            }
            if stdout {
                out.push_str(&String::from_utf8_lossy(&data));
                continue;
            }
            let Some(target) = strip_gz_suffix(&path) else {
                err.push_str(&format!("{name}: {f}: unknown suffix -- ignored\n"));
                status = 2.max(status);
                continue;
            };
            if c.vfs.exists(&target) && !force {
                err.push_str(&format!(
                    "{name}: {} already exists; not overwritten\n",
                    target.rsplit('/').next().unwrap_or(&target)
                ));
                status = 2.max(status);
                continue;
            }
            if let Err(e) = c.vfs.write_as(&target, &data, &user, tick) {
                err.push_str(&format!("{name}: {e}\n"));
                status = 1;
                continue;
            }
            carry_attributes(c, &path, &target, &user, tick);
            if verbose {
                let ratio = if data.is_empty() {
                    0.0
                } else {
                    (1.0 - bytes.len() as f64 / data.len() as f64) * 100.0
                };
                err.push_str(&format!(
                    "{f}:\t{ratio:.1}% -- replaced with {}\n",
                    target.rsplit('/').next().unwrap_or(&target)
                ));
            }
            if !keep {
                let _ = c.vfs.remove_as(&path, false, &user);
            }
        } else {
            if strip_gz_suffix(&f).is_some() {
                err.push_str(&format!(
                    "{name}: {f} already has .gz suffix -- unchanged\n"
                ));
                status = 2.max(status);
                continue;
            }
            let target = format!("{path}.gz");
            if c.vfs.exists(&target) && !force {
                err.push_str(&format!(
                    "{name}: {}.gz already exists; not overwritten\n",
                    f.rsplit('/').next().unwrap_or(&f)
                ));
                status = 2.max(status);
                continue;
            }
            let mtime = c
                .vfs
                .stat(&path)
                .map(|m| EPOCH + m.modified / 1_000_000)
                .unwrap_or(EPOCH) as u32;
            let header = cw_zlib::GzHeader {
                time: mtime,
                os: 3,
                name: Some(path.rsplit('/').next().unwrap_or(&path).as_bytes().to_vec()),
                ..Default::default()
            };
            let compressed = (|| -> Result<Vec<u8>, cw_zlib::ZError> {
                let mut d =
                    cw_zlib::Deflater::new(level, 31, 8, 0, cw_zlib::HashVariant::Canonical)?;
                d.set_header(header)?;
                let mut call = 0;
                cw_zlib::deflate_all(
                    &mut d,
                    &bytes,
                    cw_zlib::Flush::Finish,
                    &cw_zlib::OutputSchedule(vec![1 << 20]),
                    &mut call,
                )
            })();
            let compressed = match compressed {
                Ok(v) => v,
                Err(e) => {
                    err.push_str(&format!("{name}: {f}: {}\n", e.message()));
                    status = 1;
                    continue;
                }
            };
            if let Err(e) = c.vfs.write_as(&target, &compressed, &user, tick) {
                err.push_str(&format!("{name}: {e}\n"));
                status = 1;
                continue;
            }
            carry_attributes(c, &path, &target, &user, tick);
            if verbose {
                let ratio = if bytes.is_empty() {
                    0.0
                } else {
                    (1.0 - compressed.len() as f64 / bytes.len() as f64) * 100.0
                };
                err.push_str(&format!("{f}:\t{ratio:.1}% -- replaced with {f}.gz\n"));
            }
            if !keep {
                let _ = c.vfs.remove_as(&path, false, &user);
            }
        }
    }
    CommandResult {
        stdout: out,
        stderr: err,
        exit_code: status,
        ..CommandResult::default()
    }
}
