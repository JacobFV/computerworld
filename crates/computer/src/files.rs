//! Copy, move, remove, link and the metadata commands, with coreutils semantics.
//!
//! Two conventions hold across every command here:
//!
//! * A diagnostic reads `command: reason: detail` and the operand is quoted the way
//!   coreutils quotes it, so a consumer can parse one shape rather than ten.
//! * There is no terminal to prompt at, so `-i` answers **no**: an existing
//!   destination is left alone and `rm -i` removes nothing. That is exactly what a
//!   real prompt does when its input is at end of file, and it is refused loudly
//!   nowhere, because a silent overwrite would be the dangerous answer.
use crate::shell::{
    apparent, classify, clock, flag, mode_string, options, value, walk_tree, Fail, ModeSpec,
};
use crate::Computer;

/// The last path component, or `/` for the root.
fn base(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("/")
        .to_string()
}
/// The parent of an absolute path, never empty.
fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((p, _)) => p.into(),
    }
}
fn join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}
/// `a` is `b` or something underneath it.
fn inside(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{}/", b.trim_end_matches('/')))
}

/// Where a copy or a move puts its sources. `-t DIR` names the directory outright,
/// `-T` forbids the directory rule, and otherwise an existing destination directory
/// takes each source under its own name.
struct Destination {
    path: String,
    into_directory: bool,
}
fn destination(
    c: &Computer,
    name: &str,
    opts: &[(char, String)],
    operands: &[String],
) -> Result<(Vec<String>, Destination), Fail> {
    if let Some(dir) = value(opts, 't') {
        if operands.is_empty() {
            return Err(Fail::usage(format!("{name}: missing file operand")));
        }
        let path = c.resolve(dir);
        if !c.vfs.stat(&path).map(|m| m.is_dir).unwrap_or(false) {
            return Err(format!("target '{dir}': Not a directory").into());
        }
        return Ok((
            operands.to_vec(),
            Destination {
                path,
                into_directory: true,
            },
        ));
    }
    let Some((last, sources)) = operands.split_last() else {
        return Err(Fail::usage(format!("{name}: missing file operand")));
    };
    if sources.is_empty() {
        return Err(Fail::usage(format!(
            "{name}: missing destination file operand after '{last}'"
        )));
    }
    let path = c.resolve(last);
    let is_dir = c.vfs.stat(&path).map(|m| m.is_dir).unwrap_or(false);
    if flag(opts, 'T') {
        if sources.len() > 1 {
            return Err(Fail::usage(format!(
                "{name}: extra operand '{}'",
                sources[1]
            )));
        }
        return Ok((
            sources.to_vec(),
            Destination {
                path,
                into_directory: false,
            },
        ));
    }
    if sources.len() > 1 && !is_dir {
        return Err(format!("target '{last}': Not a directory").into());
    }
    Ok((
        sources.to_vec(),
        Destination {
            path,
            into_directory: is_dir,
        },
    ))
}

/// `cp`. `-a` is `-dR -p`; `-p` carries mode and timestamps across; `-d` keeps a
/// symlink a symlink instead of copying what it points at.
pub(crate) fn cp(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "cp",
        args,
        "rRafpnivdLPT",
        "t",
        &[
            ("recursive", 'r'),
            ("archive", 'a'),
            ("force", 'f'),
            ("preserve", 'p'),
            ("interactive", 'i'),
            ("no-clobber", 'n'),
            ("verbose", 'v'),
            ("no-dereference", 'd'),
            ("dereference", 'L'),
            ("target-directory", 't'),
            ("no-target-directory", 'T'),
        ],
    )?;
    let archive = flag(&opts, 'a');
    let recursive = archive || flag(&opts, 'r') || flag(&opts, 'R');
    let preserve = archive || flag(&opts, 'p');
    let keep_links = (archive || flag(&opts, 'd') || flag(&opts, 'P')) && !flag(&opts, 'L');
    let (sources, dest) = destination(c, "cp", &opts, &operands)?;
    let mut out = String::new();
    for source in &sources {
        let from = c.resolve(source);
        let to = if dest.into_directory {
            join(&dest.path, &base(&from))
        } else {
            dest.path.clone()
        };
        copy_node(
            c,
            &from,
            &to,
            source,
            CopyOptions {
                recursive,
                preserve,
                keep_links,
                interactive: flag(&opts, 'i'),
                no_clobber: flag(&opts, 'n'),
                force: flag(&opts, 'f'),
                verbose: flag(&opts, 'v'),
            },
            t,
            &mut out,
        )?;
    }
    Ok(out)
}
#[derive(Clone, Copy)]
struct CopyOptions {
    recursive: bool,
    preserve: bool,
    keep_links: bool,
    interactive: bool,
    no_clobber: bool,
    force: bool,
    verbose: bool,
}
fn copy_node(
    c: &mut Computer,
    from: &str,
    to: &str,
    label: &str,
    o: CopyOptions,
    t: u64,
    out: &mut String,
) -> Result<(), Fail> {
    let meta = c
        .vfs
        .lstat(from)
        .map_err(|_| Fail::from(format!("cannot stat '{label}': No such file or directory")))?;
    if meta.is_dir && !o.recursive {
        return Err(format!("-r not specified; omitting directory '{label}'").into());
    }
    if inside(to, from) && meta.is_dir {
        return Err(format!("cannot copy a directory, '{label}', into itself, '{to}'").into());
    }
    let exists = c.vfs.lstat(to).is_ok();
    if exists && (o.no_clobber || o.interactive) && !meta.is_dir {
        return Ok(());
    }
    if exists && o.force && !meta.is_dir && !c.vfs.lstat(to).map(|m| m.is_dir).unwrap_or(false) {
        let _ = c.vfs.remove_as(to, false, &c.user);
    }
    if meta.is_symlink && o.keep_links {
        let target = c.vfs.read_link(from).map_err(Fail::from)?;
        if exists {
            c.vfs.remove_as(to, false, &c.user).map_err(Fail::from)?;
        }
        c.vfs
            .symlink_as(&target, to, &c.user, t)
            .map_err(Fail::from)?;
    } else if meta.is_dir {
        if !c.vfs.exists(to) {
            c.vfs.mkdir_all_as(to, &c.user, t).map_err(Fail::from)?;
        } else if !c.vfs.stat(to).map(|m| m.is_dir).unwrap_or(false) {
            return Err(
                format!("cannot overwrite non-directory '{to}' with directory '{label}'").into(),
            );
        }
        for name in c.vfs.list_as(from, &c.user).map_err(Fail::from)? {
            copy_node(
                c,
                &join(from, &name),
                &join(to, &name),
                &join(label, &name),
                o,
                t,
                out,
            )?;
        }
    } else {
        let bytes = c.vfs.read_as(from, &c.user).map_err(Fail::from)?;
        c.vfs.write_as(to, &bytes, &c.user, t).map_err(Fail::from)?;
        if !o.preserve {
            // A fresh copy is born with the umask applied, but coreutils keeps the
            // source's execute bits even without -p.
            let mode = c.vfs.lstat(to).map_err(Fail::from)?.mode;
            let wanted = (mode & !0o111) | (meta.mode & 0o111 & mode_mask(mode));
            if wanted != mode {
                let _ = c.vfs.chmod_as(to, wanted, &c.user);
            }
        }
    }
    if o.preserve {
        c.vfs
            .copy_attributes_as(from, to, &c.user)
            .map_err(Fail::from)?;
    }
    if o.verbose {
        out.push_str(&format!("'{label}' -> '{to}'\n"));
    }
    Ok(())
}
/// Execute bits a copy may keep: only where the corresponding read bit survived the
/// umask, which is how `cp` of a `755` file lands as `755` and of a `700` file as `700`.
fn mode_mask(mode: u16) -> u16 {
    let mut mask = 0;
    for shift in [6, 3, 0] {
        if (mode >> shift) & 4 != 0 {
            mask |= 1 << shift;
        }
    }
    mask
}

/// `mv`. A rename within the VFS, with the directory rule and the overwrite rules
/// coreutils applies.
pub(crate) fn mv(c: &mut Computer, args: &[String], _t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "mv",
        args,
        "ifnvT",
        "t",
        &[
            ("interactive", 'i'),
            ("force", 'f'),
            ("no-clobber", 'n'),
            ("verbose", 'v'),
            ("target-directory", 't'),
            ("no-target-directory", 'T'),
        ],
    )?;
    let (sources, dest) = destination(c, "mv", &opts, &operands)?;
    let mut out = String::new();
    for source in &sources {
        let from = c.resolve(source);
        let meta = c.vfs.lstat(&from).map_err(|_| {
            Fail::from(format!("cannot stat '{source}': No such file or directory"))
        })?;
        let to = if dest.into_directory {
            join(&dest.path, &base(&from))
        } else {
            dest.path.clone()
        };
        if from == to {
            return Err(format!("'{source}' and '{to}' are the same file").into());
        }
        if let Ok(existing) = c.vfs.lstat(&to) {
            if flag(&opts, 'n') || (flag(&opts, 'i') && !flag(&opts, 'f')) {
                continue;
            }
            if existing.is_dir && !meta.is_dir {
                return Err(format!("cannot overwrite directory '{to}' with non-directory").into());
            }
            if !existing.is_dir && meta.is_dir {
                return Err(format!("cannot overwrite non-directory '{to}' with directory").into());
            }
            if existing.is_dir && meta.is_dir && !c.vfs.list(&to).unwrap_or_default().is_empty() {
                return Err(
                    format!("cannot move '{source}' to '{to}': Directory not empty").into(),
                );
            }
        }
        c.vfs.rename_as(&from, &to, &c.user).map_err(Fail::from)?;
        if flag(&opts, 'v') {
            out.push_str(&format!("renamed '{source}' -> '{to}'\n"));
        }
    }
    Ok(out)
}

/// `rm`. A directory needs `-r` (any depth) or `-d` (empty only); without either the
/// refusal is the real one, so a consumer learns the rule instead of losing a tree.
pub(crate) fn rm(c: &mut Computer, args: &[String]) -> Result<String, Fail> {
    // PowerShell's `Remove-Item -Recurse -Force` is the same command under another
    // spelling; the switches are translated before the options are parsed.
    let (opts, paths) = options(
        "rm",
        &crate::shell::powershell_switches(args),
        "rRfdivI",
        "",
        &[
            ("recursive", 'r'),
            ("force", 'f'),
            ("dir", 'd'),
            ("interactive", 'i'),
            ("verbose", 'v'),
            ("one-file-system", 'I'),
        ],
    )?;
    let recursive = flag(&opts, 'r') || flag(&opts, 'R');
    let force = flag(&opts, 'f');
    if paths.is_empty() {
        if force {
            return Ok(String::new());
        }
        return Err(Fail::usage("rm: missing operand"));
    }
    // No terminal: `-i` answers no, so nothing is removed.
    if flag(&opts, 'i') {
        return Ok(String::new());
    }
    let mut out = String::new();
    for p in &paths {
        let path = c.resolve(p);
        let Ok(meta) = c.vfs.lstat(&path) else {
            if force {
                continue;
            }
            return Err(format!("cannot remove '{p}': No such file or directory").into());
        };
        if meta.is_dir && !recursive {
            if !flag(&opts, 'd') {
                return Err(format!("cannot remove '{p}': Is a directory").into());
            }
            if !c.vfs.list(&path).unwrap_or_default().is_empty() {
                return Err(format!("cannot remove '{p}': Directory not empty").into());
            }
        }
        match c.vfs.remove_as(&path, recursive, &c.user) {
            Ok(()) => {
                if flag(&opts, 'v') {
                    out.push_str(&format!("removed '{p}'\n"));
                }
            }
            Err(e) if force => {
                let _ = e;
            }
            Err(e) => return Err(format!("cannot remove '{p}': {e}").into()),
        }
    }
    Ok(out)
}

/// `rmdir`: empty directories only, and `-p` walks up removing parents that the
/// removal just emptied.
pub(crate) fn rmdir(c: &mut Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, paths) = options(
        "rmdir",
        args,
        "pv",
        "",
        &[
            ("parents", 'p'),
            ("verbose", 'v'),
            ("ignore-fail-on-non-empty", 'e'),
        ],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("rmdir: missing operand"));
    }
    let mut out = String::new();
    for p in &paths {
        let mut path = c.resolve(p);
        let mut label = p.clone();
        loop {
            let Ok(meta) = c.vfs.lstat(&path) else {
                return Err(
                    format!("failed to remove '{label}': No such file or directory").into(),
                );
            };
            if !meta.is_dir {
                return Err(format!("failed to remove '{label}': Not a directory").into());
            }
            if !c.vfs.list(&path).unwrap_or_default().is_empty() {
                if flag(&opts, 'e') {
                    break;
                }
                return Err(format!("failed to remove '{label}': Directory not empty").into());
            }
            c.vfs
                .remove_as(&path, false, &c.user)
                .map_err(|e| Fail::from(format!("failed to remove '{label}': {e}")))?;
            if flag(&opts, 'v') {
                out.push_str(&format!("removing directory, '{label}'\n"));
            }
            if !flag(&opts, 'p') || path == "/" {
                break;
            }
            path = parent(&path);
            label = label
                .trim_end_matches('/')
                .rsplit_once('/')
                .map(|(a, _)| a.to_string())
                .unwrap_or_default();
            if label.is_empty() || path == "/" {
                break;
            }
        }
    }
    Ok(out)
}

/// `mkdir`, with `-m` applied to the directories this call creates (and, as coreutils
/// does, only to the last component when `-p` made several).
pub(crate) fn mkdir(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, paths) = options(
        "mkdir",
        args,
        "pv",
        "m",
        &[("parents", 'p'), ("verbose", 'v'), ("mode", 'm')],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("mkdir: missing operand"));
    }
    let mode = match value(&opts, 'm') {
        Some(spec) => Some(
            ModeSpec::parse(spec)
                .map_err(|_| Fail::usage(format!("mkdir: invalid mode '{spec}'")))?,
        ),
        None => None,
    };
    let mut out = String::new();
    for p in &paths {
        let path = c.resolve(p);
        if !flag(&opts, 'p') {
            if c.vfs.lstat(&path).is_ok() {
                return Err(format!("cannot create directory '{p}': File exists").into());
            }
            if !c.vfs.exists(&parent(&path)) {
                return Err(
                    format!("cannot create directory '{p}': No such file or directory").into(),
                );
            }
        } else if c.vfs.stat(&path).map(|m| m.is_dir).unwrap_or(false) {
            continue;
        }
        c.vfs.mkdir_all_as(&path, &c.user, t).map_err(Fail::from)?;
        if let Some(spec) = &mode {
            let current = c.vfs.lstat(&path).map_err(Fail::from)?.mode;
            c.vfs
                .chmod_as(&path, spec.apply(current, true), &c.user)
                .map_err(Fail::from)?;
        }
        if flag(&opts, 'v') {
            out.push_str(&format!("created directory '{p}'\n"));
        }
    }
    Ok(out)
}

/// `ln`, hard and symbolic. `-r` writes the target as a path relative to the link's own
/// directory, which is what makes a tree of links survive being moved together.
pub(crate) fn ln(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "ln",
        args,
        "sfnvrTP",
        "t",
        &[
            ("symbolic", 's'),
            ("force", 'f'),
            ("no-dereference", 'n'),
            ("verbose", 'v'),
            ("relative", 'r'),
            ("target-directory", 't'),
            ("no-target-directory", 'T'),
            ("physical", 'P'),
        ],
    )?;
    let symbolic = flag(&opts, 's');
    // `ln TARGET` with no link name puts the link in the working directory.
    let operands = if operands.len() == 1 && value(&opts, 't').is_none() {
        vec![operands[0].clone(), base(&c.resolve(&operands[0]))]
    } else {
        operands
    };
    let (targets, dest) = destination(c, "ln", &opts, &operands)?;
    // A link whose name is an existing symlink to a directory would otherwise be
    // followed into it; `-n` and `-T` say to replace the link itself.
    let into_directory = dest.into_directory
        && !(flag(&opts, 'n')
            && c.vfs
                .lstat(&dest.path)
                .map(|m| m.is_symlink)
                .unwrap_or(false));
    let mut out = String::new();
    for target in &targets {
        let link = if into_directory {
            join(&dest.path, &base(&c.resolve(target)))
        } else {
            dest.path.clone()
        };
        if c.vfs.lstat(&link).is_ok() {
            if !flag(&opts, 'f') {
                return Err(format!("failed to create link '{link}': File exists").into());
            }
            c.vfs
                .remove_as(&link, false, &c.user)
                .map_err(|e| Fail::from(format!("cannot remove '{link}': {e}")))?;
        }
        if symbolic {
            let text = if flag(&opts, 'r') {
                relative(&parent(&link), &c.resolve(target))
            } else {
                target.clone()
            };
            c.vfs
                .symlink_as(&text, &link, &c.user, t)
                .map_err(Fail::from)?;
        } else {
            let from = c.resolve(target);
            if c.vfs.lstat(&from).map(|m| m.is_dir).unwrap_or(false) {
                return Err(format!("{target}: hard link not allowed for directory").into());
            }
            c.vfs
                .hard_link_as(&from, &link, &c.user)
                .map_err(|e| Fail::from(format!("failed to create hard link '{link}': {e}")))?;
        }
        if flag(&opts, 'v') {
            out.push_str(&format!("'{link}' -> '{target}'\n"));
        }
    }
    Ok(out)
}
/// `to` expressed relative to the directory `from`, as `ln -r` and `realpath
/// --relative-to` write it.
fn relative(from: &str, to: &str) -> String {
    let a: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let shared = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), a.len() - shared).collect();
    parts.extend(b[shared..].iter().map(|s| s.to_string()));
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// `install`: copy with a mode, creating the destination directory when asked. The
/// ownership flags are refused rather than half-honoured.
pub(crate) fn install(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "install",
        args,
        "dDvpTC",
        "mt",
        &[
            ("directory", 'd'),
            ("verbose", 'v'),
            ("preserve-timestamps", 'p'),
            ("mode", 'm'),
            ("target-directory", 't'),
            ("no-target-directory", 'T'),
            ("compare", 'C'),
        ],
    )?;
    let mode = match value(&opts, 'm') {
        Some(spec) => ModeSpec::parse(spec)
            .map_err(|_| Fail::usage(format!("install: invalid mode '{spec}'")))?,
        None => ModeSpec::Absolute(0o755),
    };
    let mut out = String::new();
    if flag(&opts, 'd') {
        for p in &operands {
            let path = c.resolve(p);
            c.vfs.mkdir_all_as(&path, &c.user, t).map_err(Fail::from)?;
            let current = c.vfs.lstat(&path).map_err(Fail::from)?.mode;
            c.vfs
                .chmod_as(&path, mode.apply(current, true), &c.user)
                .map_err(Fail::from)?;
            if flag(&opts, 'v') {
                out.push_str(&format!("creating directory '{p}'\n"));
            }
        }
        return Ok(out);
    }
    let (sources, dest) = destination(c, "install", &opts, &operands)?;
    for source in &sources {
        let from = c.resolve(source);
        let to = if dest.into_directory {
            join(&dest.path, &base(&from))
        } else {
            dest.path.clone()
        };
        if flag(&opts, 'D') {
            c.vfs
                .mkdir_all_as(&parent(&to), &c.user, t)
                .map_err(Fail::from)?;
        }
        let bytes = c.vfs.read_as(&from, &c.user).map_err(|_| {
            Fail::from(format!("cannot stat '{source}': No such file or directory"))
        })?;
        c.vfs
            .write_as(&to, &bytes, &c.user, t)
            .map_err(Fail::from)?;
        if flag(&opts, 'p') {
            c.vfs
                .copy_attributes_as(&from, &to, &c.user)
                .map_err(Fail::from)?;
        }
        let current = c.vfs.lstat(&to).map_err(Fail::from)?.mode;
        c.vfs
            .chmod_as(&to, mode.apply(current, false), &c.user)
            .map_err(Fail::from)?;
        if flag(&opts, 'v') {
            out.push_str(&format!("'{source}' -> '{to}'\n"));
        }
    }
    Ok(out)
}

/// A `truncate -s` size: `N`, `+N`, `-N`, `%N` or `/N`, with the usual K/M/G suffixes.
fn size_spec(spec: &str, current: u64) -> Result<u64, Fail> {
    let invalid = || Fail::usage(format!("truncate: invalid number '{spec}'"));
    let (op, rest) = match spec.chars().next() {
        Some(c @ ('+' | '-' | '<' | '>' | '/' | '%')) => (c, &spec[1..]),
        _ => ('=', spec),
    };
    let (digits, scale) = match rest.chars().last() {
        Some('K') | Some('k') => (&rest[..rest.len() - 1], 1024u64),
        Some('M') => (&rest[..rest.len() - 1], 1024 * 1024),
        Some('G') => (&rest[..rest.len() - 1], 1024 * 1024 * 1024),
        _ => (rest, 1),
    };
    let n: u64 = digits.parse().map_err(|_| invalid())?;
    let n = n.checked_mul(scale).ok_or_else(invalid)?;
    Ok(match op {
        '+' => current.saturating_add(n),
        '-' => current.saturating_sub(n),
        '<' => current.min(n),
        '>' => current.max(n),
        '/' if n != 0 => current / n * n,
        '%' if n != 0 => current.div_ceil(n) * n,
        '/' | '%' => return Err(invalid()),
        _ => n,
    })
}
/// `truncate -s SIZE FILE…`. Growing pads with zero bytes, as `ftruncate` does.
pub(crate) fn truncate(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, paths) = options(
        "truncate",
        args,
        "c",
        "sr",
        &[("no-create", 'c'), ("size", 's'), ("reference", 'r')],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("truncate: missing file operand"));
    }
    let reference = match value(&opts, 'r') {
        Some(r) => Some(
            c.vfs
                .stat(&c.resolve(r))
                .map_err(|_| Fail::from(format!("cannot stat '{r}': No such file or directory")))?
                .size as u64,
        ),
        None => None,
    };
    let spec = value(&opts, 's');
    if spec.is_none() && reference.is_none() {
        return Err(Fail::usage("truncate: you must specify either -s or -r"));
    }
    for p in &paths {
        let path = c.resolve(p);
        let exists = c.vfs.lstat(&path).is_ok();
        if !exists && flag(&opts, 'c') {
            continue;
        }
        let current = if exists {
            c.vfs.stat(&path).map_err(Fail::from)?.size as u64
        } else {
            0
        };
        let size = match (spec, reference) {
            (Some(s), _) => size_spec(s, reference.unwrap_or(current))?,
            (None, Some(r)) => r,
            (None, None) => unreachable!(),
        };
        c.vfs
            .truncate_as(&path, size as usize, &c.user, t)
            .map_err(Fail::from)?;
    }
    Ok(String::new())
}

/// `touch`. `-a` and `-m` now select real, separate fields.
pub(crate) fn touch(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, paths) = options(
        "touch",
        args,
        "acmh",
        "dtr",
        &[
            ("no-create", 'c'),
            ("date", 'd'),
            ("reference", 'r'),
            ("no-dereference", 'h'),
        ],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("touch: missing operand"));
    }
    let (reference_a, reference_m) = match value(&opts, 'r') {
        Some(r) => {
            let m = c
                .vfs
                .lstat(&c.resolve(r))
                .map_err(|_| Fail::from(format!("cannot stat '{r}': No such file or directory")))?;
            (Some(m.accessed), Some(m.modified))
        }
        None => (None, None),
    };
    let stamp = match (value(&opts, 'd'), value(&opts, 't')) {
        (Some(date), _) => Some(crate::shell::touch_date(date)?),
        (_, Some(s)) => Some(crate::shell::touch_stamp(s)?),
        _ => None,
    };
    // Neither -a nor -m means both, as coreutils defines it.
    let (both, want_a, want_m) = {
        let (a, m) = (flag(&opts, 'a'), flag(&opts, 'm'));
        (!a && !m, a, m)
    };
    let follow = !flag(&opts, 'h');
    for operand in &paths {
        let path = c.resolve(operand);
        if !c.vfs.exists(&path) {
            if flag(&opts, 'c') {
                continue;
            }
            let birth = stamp.unwrap_or(t);
            c.vfs
                .write_as(&path, b"", &c.user, birth)
                .map_err(Fail::from)?;
        }
        let current = c.vfs.lstat(&path).map_err(Fail::from)?;
        let accessed = (both || want_a)
            .then(|| stamp.or(reference_a).unwrap_or(t))
            .or(Some(current.accessed));
        let modified = (both || want_m)
            .then(|| stamp.or(reference_m).unwrap_or(t))
            .or(Some(current.modified));
        c.vfs
            .set_times_as(&path, accessed, modified, &c.user, t, follow)
            .map_err(Fail::from)?;
    }
    Ok(String::new())
}

/// `chown [OWNER][:[GROUP]]` and `chgrp GROUP`.
pub(crate) fn chown(c: &mut Computer, cmd: &str, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, rest) = options(
        cmd,
        args,
        "Rvhc",
        "",
        &[
            ("recursive", 'R'),
            ("verbose", 'v'),
            ("no-dereference", 'h'),
            ("changes", 'c'),
        ],
    )?;
    let Some((spec, paths)) = rest.split_first() else {
        return Err(Fail::usage(format!("{cmd}: missing operand")));
    };
    if paths.is_empty() {
        return Err(Fail::usage(format!(
            "{cmd}: missing operand after '{spec}'"
        )));
    }
    let (owner, group) = if cmd == "chgrp" {
        (None, Some(spec.as_str()))
    } else {
        match spec.split_once(':') {
            // `chown alice:` means "alice and alice's login group", which here is the
            // group that carries her name.
            Some((o, "")) => (Some(o), Some(o)),
            Some(("", g)) => (None, Some(g)),
            Some((o, g)) => (Some(o), Some(g)),
            None => (Some(spec.as_str()), None),
        }
    };
    let follow = !flag(&opts, 'h');
    let mut out = String::new();
    for operand in paths {
        let root = c.resolve(operand);
        let meta = c.vfs.lstat(&root).map_err(|_| {
            Fail::from(format!(
                "cannot access '{operand}': No such file or directory"
            ))
        })?;
        let targets: Vec<String> = if flag(&opts, 'R') && meta.is_dir {
            walk_tree(c, &root).into_iter().map(|(p, _, _)| p).collect()
        } else {
            vec![root]
        };
        for path in targets {
            c.vfs
                .chown_as(&path, owner, group, &c.user, t, follow)
                .map_err(|e| Fail::from(format!("changing ownership of '{path}': {e}")))?;
            if flag(&opts, 'v') {
                let m = c.vfs.lstat(&path).map_err(Fail::from)?;
                out.push_str(&format!(
                    "ownership of '{path}' retained as {}:{}\n",
                    m.owner, m.group
                ));
            }
        }
    }
    Ok(out)
}

/// `umask [-S] [MASK]`: one mask per filesystem, read by every creation.
pub(crate) fn umask(c: &mut Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options("umask", args, "Sp", "", &[("symbolic", 'S')])?;
    let Some(spec) = rest.first() else {
        let mask = c.vfs.umask();
        return Ok(if flag(&opts, 'S') {
            format!("{}\n", symbolic_mask(mask))
        } else {
            format!("{mask:04o}\n")
        });
    };
    let mask = if spec.chars().all(|ch| ch.is_ascii_digit()) {
        u16::from_str_radix(spec, 8)
            .ok()
            .filter(|m| *m <= 0o777)
            .ok_or_else(|| Fail::usage(format!("umask: '{spec}': invalid octal number")))?
    } else {
        // A symbolic mask names the bits to KEEP, so it applies to the complement.
        let spec = ModeSpec::parse(spec)
            .map_err(|_| Fail::usage(format!("umask: '{spec}': invalid symbolic mode")))?;
        0o777 & !spec.apply(0o777 & !c.vfs.umask(), false)
    };
    c.vfs.set_umask(mask);
    Ok(String::new())
}
fn symbolic_mask(mask: u16) -> String {
    let keep = 0o777 & !mask;
    ["u", "g", "o"]
        .iter()
        .enumerate()
        .map(|(i, who)| {
            let bits = (keep >> (6 - 3 * i)) & 7;
            let mut s = String::from(*who);
            s.push('=');
            for (bit, ch) in [(4, 'r'), (2, 'w'), (1, 'x')] {
                if bits & bit != 0 {
                    s.push(ch);
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// `readlink [-f]` and `realpath`: the stored link text, or the path with every
/// symbolic link on it resolved.
pub(crate) fn readlink(c: &Computer, cmd: &str, args: &[String]) -> Result<String, Fail> {
    let (opts, paths) = options(
        cmd,
        args,
        "fems",
        "",
        &[
            ("canonicalize", 'f'),
            ("canonicalize-existing", 'e'),
            ("canonicalize-missing", 'm'),
            ("silent", 's'),
            ("quiet", 's'),
        ],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage(format!("{cmd}: missing operand")));
    }
    let canonical = cmd == "realpath" || flag(&opts, 'f') || flag(&opts, 'e') || flag(&opts, 'm');
    let mut out = String::new();
    for p in &paths {
        let path = c.resolve(p);
        if !canonical {
            let target = c
                .vfs
                .read_link(&path)
                .map_err(|_| Fail::new(String::new(), 1))?;
            out.push_str(&format!("{target}\n"));
            continue;
        }
        let resolved = canonicalize(c, &path)?;
        if !flag(&opts, 'm') && c.vfs.lstat(&resolved).is_err() {
            return Err(format!("{p}: No such file or directory").into());
        }
        out.push_str(&format!("{resolved}\n"));
    }
    Ok(out)
}
/// Resolve every symbolic link on a path, bounded the way the VFS bounds a lookup.
fn canonicalize(c: &Computer, path: &str) -> Result<String, Fail> {
    let mut current = String::from("/");
    for part in path.split('/').filter(|s| !s.is_empty()) {
        current = crate::normalize_path(&current, part);
        let mut hops = 0;
        while let Ok(target) = c.vfs.read_link(&current) {
            hops += 1;
            if hops > 40 {
                return Err("Too many levels of symbolic links".into());
            }
            current = crate::normalize_path(&parent(&current), &target);
        }
    }
    Ok(current)
}

/// `--color` and `--json` have no short spelling in coreutils, and inventing one
/// would collide with `-c` (sort by change time) and shadow a real flag. They are
/// carried on control characters instead, which no argument can contain.
const COLOUR_FLAG: char = '\u{1}';
const JSON_FLAG: char = '\u{2}';
const COLOUR: &str = "\u{1}";
/// The `ls` flag set, resolved once so the renderer never re-reads the option list.
pub(crate) struct LsFlags {
    pub all: bool,
    pub almost: bool,
    pub long: bool,
    pub human: bool,
    pub classify: bool,
    /// `-p`: a slash after a directory, and nothing else.
    pub slash: bool,
    pub reverse: bool,
    pub by_time: bool,
    pub by_size: bool,
    pub inode: bool,
    pub numeric: bool,
    pub json: bool,
}
/// The `ls --json` schema, documented in docs/shell.md: one object per entry with
/// `name`, `kind`, `size`, `mode`, `mtime` and `target`. This is the answer to the
/// N+1 `test -d` storm — one call tells an agent what every entry is.
fn json_entry(c: &Computer, name: &str, path: &str) -> String {
    let Ok(m) = c.vfs.lstat(path) else {
        return format!("{{\"name\":{},\"kind\":\"unknown\"}}", json_string(name));
    };
    let kind = if m.is_symlink {
        "symlink"
    } else if m.is_dir {
        "directory"
    } else {
        "file"
    };
    let target = match &m.target {
        Some(t) => format!(",\"target\":{}", json_string(t)),
        None => String::new(),
    };
    format!(
        "{{\"name\":{},\"path\":{},\"kind\":\"{kind}\",\"size\":{},\"mode\":\"{:04o}\",\
         \"owner\":{},\"group\":{},\"links\":{},\"inode\":{},\"atime\":\"{}\",\"mtime\":\"{}\",\
         \"ctime\":\"{}\"{target}}}",
        json_string(name),
        json_string(path),
        apparent(&m),
        m.mode & 0o7777,
        json_string(&m.owner),
        json_string(&m.group),
        m.links,
        m.inode,
        clock(m.accessed).stamp(),
        clock(m.modified).stamp(),
        clock(m.changed).stamp(),
    )
}
pub(crate) fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
/// Entries are (label, absolute path) so an operand keeps the spelling the caller used.
fn ls_render(c: &Computer, entries: &[(String, String)], f: &LsFlags) -> String {
    let rows: Vec<(String, Option<crate::Metadata>)> = entries
        .iter()
        .map(|(n, path)| (n.clone(), c.vfs.lstat(path).ok()))
        .collect();
    if f.json {
        let body: Vec<String> = entries.iter().map(|(n, p)| json_entry(c, n, p)).collect();
        return format!("[{}]\n", body.join(","));
    }
    let mark = |m: &Option<crate::Metadata>| match m {
        Some(meta) if f.classify => classify(meta),
        Some(meta) if f.slash && meta.is_dir => '/',
        _ => ' ',
    };
    let inode = |m: &Option<crate::Metadata>| match (f.inode, m) {
        (true, Some(meta)) => format!("{} ", meta.inode),
        (true, None) => "? ".to_string(),
        _ => String::new(),
    };
    if !f.long {
        return rows
            .iter()
            .map(|(n, m)| {
                let mark = mark(m);
                format!(
                    "{}{n}{}\n",
                    inode(m),
                    if mark == ' ' {
                        String::new()
                    } else {
                        mark.to_string()
                    }
                )
            })
            .collect();
    }
    let blocks: u64 = rows
        .iter()
        .filter_map(|(_, m)| m.as_ref())
        .map(|m| crate::shell::allocated(apparent(m)) / 1024)
        .sum();
    let sized: Vec<String> = rows
        .iter()
        .map(|(_, m)| match m {
            Some(meta) if f.human => crate::shell::human(apparent(meta)),
            Some(meta) => apparent(meta).to_string(),
            None => "?".into(),
        })
        .collect();
    let width = sized.iter().map(String::len).max().unwrap_or(1);
    let name_of = |m: &crate::Metadata, group: bool| {
        if f.numeric {
            (if group {
                c.hardware.gid
            } else {
                c.hardware.uid
            })
            .to_string()
        } else if group {
            m.group.clone()
        } else {
            m.owner.clone()
        }
    };
    let owners = rows
        .iter()
        .filter_map(|(_, m)| m.as_ref())
        .map(|m| name_of(m, false).len())
        .max()
        .unwrap_or(1);
    let groups = rows
        .iter()
        .filter_map(|(_, m)| m.as_ref())
        .map(|m| name_of(m, true).len())
        .max()
        .unwrap_or(1);
    let links = rows
        .iter()
        .filter_map(|(_, m)| m.as_ref())
        .map(|m| m.links.to_string().len())
        .max()
        .unwrap_or(1);
    let mut out = format!("total {blocks}\n");
    for ((name, meta), size) in rows.iter().zip(sized) {
        let Some(m) = meta else {
            out.push_str(&format!("?????????? ? ? ? {size:>width$} ? {name}\n"));
            continue;
        };
        let target = match &m.target {
            Some(t) => format!(" -> {t}"),
            None => String::new(),
        };
        let mark = mark(&Some(m.clone()));
        out.push_str(&format!(
            "{}{} {:>links$} {:<owners$} {:<groups$} {:>width$} {} {name}{}{}\n",
            if f.inode {
                format!("{} ", m.inode)
            } else {
                String::new()
            },
            mode_string(m.mode, m.is_dir, m.is_symlink),
            m.links,
            name_of(m, false),
            name_of(m, true),
            size,
            clock(m.modified).short(),
            if mark == ' ' {
                String::new()
            } else {
                mark.to_string()
            },
            target,
        ));
    }
    out
}
fn ls_block(c: &Computer, dir: &str, f: &LsFlags) -> Result<String, Fail> {
    let mut names = c.vfs.list_as(dir, &c.user)?;
    if !f.all && !f.almost {
        names.retain(|n| !n.starts_with('.'));
    }
    let meta = |n: &String| c.vfs.lstat(&crate::normalize_path(dir, n));
    if f.by_time {
        names.sort_by_key(|n| std::cmp::Reverse(meta(n).map(|m| m.modified).unwrap_or(0)));
    } else if f.by_size {
        names.sort_by_key(|n| std::cmp::Reverse(meta(n).map(|m| apparent(&m)).unwrap_or(0)));
    }
    if f.reverse {
        names.reverse();
    }
    if f.all {
        names.splice(0..0, [".".to_string(), "..".to_string()]);
    }
    let entries: Vec<(String, String)> = names
        .iter()
        .map(|n| (n.clone(), crate::normalize_path(dir, n)))
        .collect();
    Ok(ls_render(c, &entries, f))
}
pub(crate) fn ls(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, mut paths) = options(
        "ls",
        args,
        "aAldh1FpirtSRn",
        COLOUR,
        &[
            ("all", 'a'),
            ("almost-all", 'A'),
            ("human-readable", 'h'),
            ("reverse", 'r'),
            ("recursive", 'R'),
            ("directory", 'd'),
            ("classify", 'F'),
            ("inode", 'i'),
            ("numeric-uid-gid", 'n'),
            ("color", COLOUR_FLAG),
            ("json", JSON_FLAG),
        ],
    )?;
    // The only colour this world has is none; anything else would be a lie about a
    // terminal it cannot see.
    if let Some(when) = value(&opts, COLOUR_FLAG) {
        if !matches!(when, "never" | "no" | "none" | "auto" | "") {
            return Err(Fail::usage(format!(
                "ls: unsupported --color mode `{when}`"
            )));
        }
    }
    let f = LsFlags {
        all: flag(&opts, 'a'),
        almost: flag(&opts, 'A'),
        long: flag(&opts, 'l'),
        human: flag(&opts, 'h'),
        classify: flag(&opts, 'F'),
        slash: flag(&opts, 'p'),
        reverse: flag(&opts, 'r'),
        by_time: flag(&opts, 't'),
        by_size: flag(&opts, 'S'),
        inode: flag(&opts, 'i'),
        numeric: flag(&opts, 'n'),
        json: flag(&opts, JSON_FLAG),
    };
    if paths.is_empty() {
        paths.push(".".into());
    }
    let mut out = String::new();
    let mut queue: Vec<(String, String)> = Vec::new();
    for operand in &paths {
        let resolved = c.resolve(operand);
        let meta = c
            .vfs
            .lstat(&resolved)
            .map_err(|_| format!("cannot access '{operand}': No such file or directory"))?;
        // `ls -l` on a symlink follows it unless the caller said -d; `ls` on a
        // directory lists it.
        let dir = if meta.is_symlink {
            c.vfs.stat(&resolved).map(|m| m.is_dir).unwrap_or(false)
        } else {
            meta.is_dir
        };
        if dir && !flag(&opts, 'd') {
            queue.push((operand.clone(), resolved));
        } else {
            out.push_str(&ls_render(c, &[(operand.clone(), resolved)], &f));
        }
    }
    let titled = queue.len() > 1 || flag(&opts, 'R') || !out.is_empty();
    while let Some((label, dir)) = queue.first().cloned() {
        queue.remove(0);
        if !out.is_empty() {
            out.push('\n');
        }
        if titled && !f.json {
            out.push_str(&format!("{label}:\n"));
        }
        out.push_str(&ls_block(c, &dir, &f)?);
        if flag(&opts, 'R') {
            let mut children: Vec<(String, String)> = c
                .vfs
                .list_as(&dir, &c.user)?
                .into_iter()
                .filter(|n| f.all || f.almost || !n.starts_with('.'))
                .map(|n| {
                    (
                        format!("{}/{n}", label.trim_end_matches('/')),
                        crate::normalize_path(&dir, &n),
                    )
                })
                .filter(|(_, p)| c.vfs.lstat(p).is_ok_and(|m| m.is_dir))
                .collect();
            children.append(&mut queue);
            queue = children;
        }
    }
    Ok(out)
}

/// `2026-09-17 09:00:00.000000000 +0000`, the stamp `stat` prints for each time.
fn stamp_of(tick: u64) -> String {
    format!("{}.000000000 +0000", clock(tick).stamp())
}
/// `stat -c` conversions over the real metadata the VFS now keeps.
fn stat_format(
    c: &Computer,
    format: &str,
    path: &str,
    m: &crate::Metadata,
) -> Result<String, Fail> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let kind = if m.is_symlink {
        "symbolic link"
    } else if m.is_dir {
        "directory"
    } else {
        "regular file"
    };
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(match chars[i + 1] {
                'n' => '\n',
                't' => '\t',
                '0' => '\0',
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
            .ok_or_else(|| Fail::usage("stat: trailing `%` in format".to_string()))?;
        let type_bits: u32 = if m.is_symlink {
            0o120000
        } else if m.is_dir {
            0o040000
        } else {
            0o100000
        };
        match spec {
            '%' => out.push('%'),
            'n' => out.push_str(path),
            'N' => out.push_str(&match &m.target {
                Some(t) => format!("'{path}' -> '{t}'"),
                None => format!("'{path}'"),
            }),
            's' => out.push_str(&apparent(m).to_string()),
            'b' => out.push_str(&apparent(m).div_ceil(512).to_string()),
            'B' => out.push_str("512"),
            'o' => out.push_str(&crate::shell::BLOCK.to_string()),
            'f' => out.push_str(&format!("{:x}", type_bits | u32::from(m.mode))),
            'a' => out.push_str(&format!("{:o}", m.mode & 0o7777)),
            'A' => out.push_str(&mode_string(m.mode, m.is_dir, m.is_symlink)),
            'F' => out.push_str(kind),
            'U' => out.push_str(&m.owner),
            'G' => out.push_str(&m.group),
            'u' => out.push_str(&c.hardware.uid.to_string()),
            'g' => out.push_str(&c.hardware.gid.to_string()),
            'i' => out.push_str(&m.inode.to_string()),
            'h' => out.push_str(&m.links.to_string()),
            'm' => out.push('/'),
            'd' => out.push('1'),
            't' | 'T' => out.push('0'),
            'X' => out.push_str(&clock(m.accessed).unix.to_string()),
            'Y' => out.push_str(&clock(m.modified).unix.to_string()),
            'Z' => out.push_str(&clock(m.changed).unix.to_string()),
            'W' => out.push_str(&clock(m.created).unix.to_string()),
            'x' => out.push_str(&stamp_of(m.accessed)),
            'y' => out.push_str(&stamp_of(m.modified)),
            'z' => out.push_str(&stamp_of(m.changed)),
            'w' => out.push_str(&stamp_of(m.created)),
            other => {
                return Err(Fail::usage(format!(
                    "stat: unsupported conversion `%{other}`"
                )))
            }
        }
        i += 2;
    }
    Ok(out)
}
/// `stat -f` conversions: the filesystem, not the file. One filesystem is mounted at
/// `/`, its capacity is `hardware.disk_bytes` and its usage is summed from the VFS.
fn statfs_format(c: &Computer, format: &str, path: &str) -> Result<String, Fail> {
    let used: u64 = walk_tree(c, "/")
        .iter()
        .map(|(_, size, _)| crate::shell::allocated(*size))
        .sum();
    let total = c.hardware.disk_bytes;
    let block = crate::shell::BLOCK;
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
            .ok_or_else(|| Fail::usage("stat: trailing `%` in format".to_string()))?;
        match spec {
            '%' => out.push('%'),
            'n' => out.push_str(path),
            'i' => out.push('0'),
            'l' => out.push_str("255"),
            'T' => out.push_str("ext2/ext3"),
            't' => out.push_str("ef53"),
            's' | 'S' => out.push_str(&block.to_string()),
            'b' => out.push_str(&(total / block).to_string()),
            'f' | 'a' => out.push_str(&(total.saturating_sub(used) / block).to_string()),
            'c' => out.push_str(&walk_tree(c, "/").len().to_string()),
            'd' => out.push('0'),
            other => {
                return Err(Fail::usage(format!(
                    "stat: unsupported filesystem conversion `%{other}`"
                )))
            }
        }
        i += 2;
    }
    Ok(out)
}
pub(crate) fn stat(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, paths) = options(
        "stat",
        args,
        "Lft",
        "c",
        &[
            ("format", 'c'),
            ("printf", 'c'),
            ("dereference", 'L'),
            ("file-system", 'f'),
            ("terse", 't'),
        ],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("stat: missing operand"));
    }
    let mut out = String::new();
    for operand in &paths {
        let path = c.resolve(operand);
        c.vfs
            .check_access(&path, &c.user, false, false, false)
            .map_err(Fail::from)?;
        if flag(&opts, 'f') {
            if c.vfs.lstat(&path).is_err() {
                return Err(format!(
                    "cannot read file system information for '{operand}': \
                                    No such file or directory"
                )
                .into());
            }
            let Some(format) = value(&opts, 'c') else {
                // The default layout is written out rather than routed through the
                // conversion engine, so no width syntax has to be invented for it.
                out.push_str(&statfs_format(
                    c,
                    "  File: \"%n\"\n    ID: 0        Namelen: %l     Type: %T\n\
                     Block size: %s       Fundamental block size: %S\n\
                     Blocks: Total: %b  Free: %f  Available: %a\n\
                     Inodes: Total: %c  Free: %d\n",
                    operand,
                )?);
                continue;
            };
            out.push_str(&statfs_format(c, format, operand)?);
            out.push('\n');
            continue;
        }
        // Without -L a symlink describes itself, exactly as coreutils does.
        let m = if flag(&opts, 'L') {
            c.vfs.stat(&path)
        } else {
            c.vfs.lstat(&path)
        }
        .map_err(Fail::from)?;
        if let Some(format) = value(&opts, 'c') {
            out.push_str(&stat_format(c, format, operand, &m)?);
            out.push('\n');
            continue;
        }
        if flag(&opts, 't') {
            out.push_str(&stat_format(
                c,
                "%n %s %b %f %u %g %d %i %h 0 0 %X %Y %Z %o",
                operand,
                &m,
            )?);
            out.push('\n');
            continue;
        }
        let kind = if m.is_symlink {
            "symbolic link"
        } else if m.is_dir {
            "directory"
        } else {
            "regular file"
        };
        let name = match &m.target {
            Some(t) => format!("{operand} -> {t}"),
            None => operand.clone(),
        };
        out.push_str(&format!(
            "  File: {name}\n  Size: {size:<10}\tBlocks: {blocks:<10} IO Block: {block:<6} {kind}\n\
             Device: 1,0\tInode: {inode:<11} Links: {links}\n\
             Access: ({mode:04o}/{rwx})  Uid: ({uid:5}/{owner:>8})   Gid: ({gid:5}/{group:>8})\n\
             Access: {atime}\nModify: {mtime}\nChange: {ctime}\n Birth: {btime}\n",
            size = apparent(&m),
            blocks = apparent(&m).div_ceil(512),
            block = crate::shell::BLOCK,
            inode = m.inode,
            links = m.links,
            mode = m.mode & 0o7777,
            rwx = mode_string(m.mode, m.is_dir, m.is_symlink),
            uid = c.hardware.uid,
            gid = c.hardware.gid,
            owner = m.owner,
            group = m.group,
            atime = stamp_of(m.accessed),
            mtime = stamp_of(m.modified),
            ctime = stamp_of(m.changed),
            btime = stamp_of(m.created),
        ));
    }
    Ok(out)
}
