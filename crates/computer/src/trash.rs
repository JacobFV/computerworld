//! A real FreeDesktop trash.
//!
//! `~/.local/share/Trash/files/NAME` holds what was deleted and
//! `~/.local/share/Trash/info/NAME.trashinfo` holds the record that makes a restore
//! possible:
//!
//! ```text
//! [Trash Info]
//! Path=/home/user/notes/todo.md
//! DeletionDate=2026-09-17T09:00:00
//! ```
//!
//! The path is percent-encoded and the date comes from the world clock, so the record
//! is byte-identical on every replay. `trash`, `trash-list`, `trash-restore`,
//! `trash-empty` and `gio trash` all read and write these files, and so do the
//! desktops' Move to Trash and Restore — one trash, one format, one source of truth.
use crate::shell::{clock, flag, options, Fail};
use crate::{Computer, Vfs, VfsError};

/// One thing in the trash, as `.trashinfo` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashEntry {
    /// The name under `files/` (and, with `.trashinfo` appended, under `info/`).
    pub name: String,
    /// Where it came from, and where a restore puts it back.
    pub original: String,
    /// World-clock stamp, `YYYY-MM-DDThh:mm:ss`.
    pub deleted: String,
    /// Absolute path of the copy now sitting in the trash.
    pub path: String,
}

/// `$HOME/.local/share/Trash` for a user whose home is `home`.
pub fn trash_root(home: &str) -> String {
    format!("{}/.local/share/Trash", home.trim_end_matches('/'))
}
pub fn files_dir(home: &str) -> String {
    format!("{}/files", trash_root(home))
}
pub fn info_dir(home: &str) -> String {
    format!("{}/info", trash_root(home))
}
fn home_of(c: &Computer) -> String {
    c.env
        .get("HOME")
        .cloned()
        .unwrap_or_else(|| format!("/home/{}", c.user))
}

/// Percent-encoding as the trash spec wants it: everything but the unreserved set and
/// the separators that make a path readable.
fn encode(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
/// `YYYY-MM-DDThh:mm:ss`, the spec's `DeletionDate`.
fn stamp(tick: u64) -> String {
    let n = clock(tick);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        n.year, n.month, n.day, n.hour, n.minute, n.second
    )
}
fn base(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("trashed")
        .to_string()
}
fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((p, _)) => p.into(),
    }
}

/// Move one path into the trash and write its record. Returns the name it took under
/// `files/`. This is the single implementation the shell, the kernel and every
/// desktop file manager go through.
pub fn put(vfs: &mut Vfs, user: &str, home: &str, path: &str, tick: u64) -> Result<String, String> {
    let path = crate::normalize_path("/", path);
    let (files, info) = (files_dir(home), info_dir(home));
    if path == trash_root(home) || path.starts_with(&format!("{}/", trash_root(home))) {
        return Err(format!("cannot trash '{path}': it is already in the trash"));
    }
    if vfs.lstat(&path).is_err() {
        return Err(format!("cannot trash '{path}': No such file or directory"));
    }
    vfs.mkdir_all_as(&files, user, tick).map_err(err)?;
    vfs.mkdir_all_as(&info, user, tick).map_err(err)?;
    // A name already in the trash keeps its own copy: `todo.md`, `todo.md.2`, …
    let wanted = base(&path);
    let mut name = wanted.clone();
    let mut n = 2;
    while vfs.lstat(&format!("{files}/{name}")).is_ok()
        || vfs.lstat(&format!("{info}/{name}.trashinfo")).is_ok()
    {
        name = format!("{wanted}.{n}");
        n += 1;
        if n > 1000 {
            return Err(format!(
                "cannot trash '{path}': too many copies of '{wanted}'"
            ));
        }
    }
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode(&path),
        stamp(tick)
    );
    vfs.rename_as(&path, &format!("{files}/{name}"), user)
        .map_err(err)?;
    vfs.write_as(
        &format!("{info}/{name}.trashinfo"),
        record.as_bytes(),
        user,
        tick,
    )
    .map_err(err)?;
    Ok(name)
}
fn err(e: VfsError) -> String {
    e.to_string()
}

/// Everything in the trash, oldest record first by name so the order never wobbles.
pub fn list(vfs: &Vfs, home: &str) -> Vec<TrashEntry> {
    let (files, info) = (files_dir(home), info_dir(home));
    let mut out = Vec::new();
    for record in vfs.list(&info).unwrap_or_default() {
        let Some(name) = record.strip_suffix(".trashinfo") else {
            continue;
        };
        let Ok(bytes) = vfs.read(&format!("{info}/{record}")) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut original = String::new();
        let mut deleted = String::new();
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("Path=") {
                original = decode(v);
            } else if let Some(v) = line.strip_prefix("DeletionDate=") {
                deleted = v.trim().to_string();
            }
        }
        if original.is_empty() {
            continue;
        }
        out.push(TrashEntry {
            name: name.to_string(),
            original,
            deleted,
            path: format!("{files}/{name}"),
        });
    }
    out
}
/// The entries whose original path matches `query`: an exact original path, a trash
/// name, or a basename. Empty `query` means everything.
pub fn matching(vfs: &Vfs, home: &str, query: &str) -> Vec<TrashEntry> {
    let all = list(vfs, home);
    if query.is_empty() {
        return all;
    }
    let exact: Vec<TrashEntry> = all
        .iter()
        .filter(|e| e.original == query || e.name == query)
        .cloned()
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let wanted = base(query);
    all.into_iter()
        .filter(|e| {
            base(&e.original) == wanted || crate::shell::wildcard(query, &base(&e.original))
        })
        .collect()
}
/// Put one entry back where it came from. Returns the path it was restored to.
pub fn restore(
    vfs: &mut Vfs,
    user: &str,
    home: &str,
    entry: &TrashEntry,
    force: bool,
    tick: u64,
) -> Result<String, String> {
    if vfs.lstat(&entry.original).is_ok() {
        if !force {
            return Err(format!(
                "cannot restore '{}': destination already exists",
                entry.original
            ));
        }
        vfs.remove_as(&entry.original, true, user).map_err(err)?;
    }
    let dir = parent(&entry.original);
    if vfs.lstat(&dir).is_err() {
        vfs.mkdir_all_as(&dir, user, tick).map_err(err)?;
    }
    vfs.rename_as(&entry.path, &entry.original, user)
        .map_err(err)?;
    let record = format!("{}/{}.trashinfo", info_dir(home), entry.name);
    let _ = vfs.remove_as(&record, false, user);
    Ok(entry.original.clone())
}
/// Throw away what is in the trash for good. Returns how many entries went.
pub fn empty(vfs: &mut Vfs, user: &str, home: &str) -> Result<usize, String> {
    let entries = list(vfs, home);
    let count = entries.len();
    for entry in entries {
        let _ = vfs.remove_as(&entry.path, true, user);
        let _ = vfs.remove_as(
            &format!("{}/{}.trashinfo", info_dir(home), entry.name),
            false,
            user,
        );
    }
    // Anything in `files/` without a record is still trash and still goes.
    for name in vfs.list(&files_dir(home)).unwrap_or_default() {
        let _ = vfs.remove_as(&format!("{}/{name}", files_dir(home)), true, user);
    }
    Ok(count)
}

/// `trash`, `trash-put`, `trash-list`, `trash-restore`, `trash-empty` and `gio trash`.
pub(crate) fn command(
    c: &mut Computer,
    cmd: &str,
    args: &[String],
    t: u64,
) -> Result<String, Fail> {
    if cmd == "gio" {
        let Some(sub) = args.first() else {
            return Err(Fail::usage(
                "gio: missing subcommand; this world implements `gio trash`",
            ));
        };
        if sub != "trash" {
            // The opener handles every other subcommand; this arm only ever sees
            // `gio trash`, so anything else here is a dispatch mistake, not a user one.
            return Err(Fail::usage(format!(
                "gio: unsupported subcommand `{sub}`; this world models `gio open` \
                 and `gio trash`"
            )));
        }
        let rest = &args[1..];
        return match rest.first().map(String::as_str) {
            Some("--list") => command(c, "trash-list", &rest[1..], t),
            Some("--empty") => command(c, "trash-empty", &rest[1..], t),
            Some("--restore") => command(c, "trash-restore", &rest[1..], t),
            _ => command(c, "trash-put", rest, t),
        };
    }
    let home = home_of(c);
    match cmd {
        "trash" | "trash-put" => {
            let (opts, paths) = options(cmd, args, "vf", "", &[("verbose", 'v'), ("force", 'f')])?;
            if paths.is_empty() {
                return Err(Fail::usage(format!("{cmd}: missing operand")));
            }
            let mut out = String::new();
            for p in &paths {
                let path = c.resolve(p);
                match put(&mut c.vfs, &c.user.clone(), &home, &path, t) {
                    Ok(name) => {
                        if flag(&opts, 'v') {
                            out.push_str(&format!(
                                "trashed '{p}' as '{}/{name}'\n",
                                files_dir(&home)
                            ));
                        }
                    }
                    Err(e) if flag(&opts, 'f') => {
                        let _ = e;
                    }
                    Err(e) => return Err(Fail::new(e, 1)),
                }
            }
            Ok(out)
        }
        "trash-list" => {
            let (opts, rest) = options("trash-list", args, "J", "", &[("json", 'J')])?;
            if let Some(extra) = rest.first() {
                return Err(Fail::usage(format!(
                    "trash-list: unexpected operand '{extra}'"
                )));
            }
            let entries = list(&c.vfs, &home);
            if flag(&opts, 'J') {
                let body: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        format!(
                            "{{\"name\":{},\"original\":{},\"deleted\":{},\"path\":{}}}",
                            crate::files::json_string(&e.name),
                            crate::files::json_string(&e.original),
                            crate::files::json_string(&e.deleted),
                            crate::files::json_string(&e.path),
                        )
                    })
                    .collect();
                return Ok(format!("[{}]\n", body.join(",")));
            }
            Ok(entries
                .iter()
                .map(|e| format!("{} {}\n", e.deleted.replace('T', " "), e.original))
                .collect())
        }
        "trash-restore" => {
            let (opts, rest) = options(
                "trash-restore",
                args,
                "af",
                "",
                &[("all", 'a'), ("force", 'f'), ("overwrite", 'f')],
            )?;
            let query = rest.first().cloned().unwrap_or_default();
            if query.is_empty() && !flag(&opts, 'a') {
                return Err(Fail::usage(
                    "trash-restore: name what to restore (an original path, a basename or \
                     a pattern), or pass --all",
                ));
            }
            let query = if query.is_empty() {
                String::new()
            } else {
                // An operand is resolved against the working directory only when it
                // looks like a path; a bare name stays a name.
                if query.contains('/') {
                    c.resolve(&query)
                } else {
                    query
                }
            };
            let entries = matching(&c.vfs, &home, &query);
            if entries.is_empty() {
                return Err(Fail::new(
                    format!("trash-restore: nothing in the trash matches '{query}'"),
                    1,
                ));
            }
            if entries.len() > 1 && !flag(&opts, 'a') {
                let mut text =
                    String::from("trash-restore: more than one entry matches; name one of:\n");
                for e in &entries {
                    text.push_str(&format!("  {} ({})\n", e.original, e.name));
                }
                return Err(Fail::new(text, 1));
            }
            let mut out = String::new();
            let user = c.user.clone();
            for entry in entries {
                let where_to = restore(&mut c.vfs, &user, &home, &entry, flag(&opts, 'f'), t)
                    .map_err(|e| Fail::new(format!("trash-restore: {e}"), 1))?;
                out.push_str(&format!("restored '{where_to}'\n"));
            }
            Ok(out)
        }
        "trash-empty" => {
            let (_, rest) = options("trash-empty", args, "f", "", &[("force", 'f')])?;
            if let Some(days) = rest.first() {
                // `trash-empty DAYS` needs an age comparison the spec stores only as a
                // stamp; refusing is better than quietly emptying everything.
                return Err(Fail::usage(format!(
                    "trash-empty: an age operand ('{days}') is not implemented; \
                     `trash-empty` with no operand empties the whole trash"
                )));
            }
            let user = c.user.clone();
            let n = empty(&mut c.vfs, &user, &home).map_err(|e| Fail::new(e, 1))?;
            Ok(if n == 0 {
                String::new()
            } else {
                format!("emptied {n} item{}\n", if n == 1 { "" } else { "s" })
            })
        }
        other => Err(Fail::usage(format!("{other}: not a trash command"))),
    }
}
#[cfg(test)]
mod tests {
    use crate::{shell, Computer, OfflineHost};
    fn machine() -> Computer {
        let mut c = Computer::new("box", "user", "linux", true);
        for line in [
            "mkdir -p /home/user/notes",
            "echo todo > /home/user/notes/todo.md",
            "echo report > /home/user/report.txt",
        ] {
            let r = shell::execute(&mut c, line, 0, &mut OfflineHost);
            assert_eq!(r.exit_code, 0, "{line}: {}", r.stderr);
        }
        c
    }
    fn run(c: &mut Computer, line: &str, t: u64) -> crate::CommandResult {
        shell::execute(c, line, t, &mut OfflineHost)
    }
    fn ok(c: &mut Computer, line: &str, t: u64) -> String {
        let r = run(c, line, t);
        assert_eq!(r.exit_code, 0, "`{line}`: {}", r.stderr);
        r.stdout
    }

    #[test]
    fn a_trashed_file_leaves_a_record_and_comes_back_where_it_was() {
        let mut c = machine();
        ok(&mut c, "trash /home/user/notes/todo.md", 0);
        // It is gone from where it was, and present in the trash.
        assert_eq!(
            run(&mut c, "test -e /home/user/notes/todo.md", 0).exit_code,
            1
        );
        assert_eq!(
            ok(&mut c, "cat /home/user/.local/share/Trash/files/todo.md", 0),
            "todo\n"
        );
        let record = ok(
            &mut c,
            "cat /home/user/.local/share/Trash/info/todo.md.trashinfo",
            0,
        );
        assert_eq!(
            record,
            "[Trash Info]\nPath=/home/user/notes/todo.md\nDeletionDate=2026-09-17T09:00:00\n"
        );
        assert_eq!(
            ok(&mut c, "trash-list", 0),
            "2026-09-17 09:00:00 /home/user/notes/todo.md\n"
        );
        // "I deleted X by mistake, restore it" — from the shell.
        assert_eq!(
            ok(&mut c, "trash-restore /home/user/notes/todo.md", 0),
            "restored '/home/user/notes/todo.md'\n"
        );
        assert_eq!(ok(&mut c, "cat /home/user/notes/todo.md", 0), "todo\n");
        assert_eq!(ok(&mut c, "trash-list", 0), "");
    }

    #[test]
    fn a_basename_is_enough_and_a_second_copy_gets_its_own_name() {
        let mut c = machine();
        ok(&mut c, "trash /home/user/notes/todo.md", 0);
        ok(&mut c, "echo second > /home/user/todo.md", 1_000_000);
        ok(&mut c, "trash /home/user/todo.md", 2_000_000);
        let names = ok(&mut c, "ls /home/user/.local/share/Trash/files", 0);
        assert_eq!(names, "todo.md\ntodo.md.2\n");
        // A pattern that matches both asks rather than guessing.
        let r = run(&mut c, "trash-restore 'todo*'", 0);
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("more than one entry"), "{}", r.stderr);
        assert!(
            r.stderr.contains("/home/user/notes/todo.md"),
            "{}",
            r.stderr
        );
        // An original path names exactly one of them.
        ok(&mut c, "trash-restore /home/user/todo.md", 0);
        assert_eq!(ok(&mut c, "cat /home/user/todo.md", 0), "second\n");
        // And so does the name the trash gave it.
        ok(&mut c, "trash-restore todo.md", 0);
        assert_eq!(ok(&mut c, "cat /home/user/notes/todo.md", 0), "todo\n");
        // --all puts everything back in one call.
        ok(&mut c, "trash /home/user/todo.md", 3_000_000);
        ok(&mut c, "trash /home/user/notes/todo.md", 4_000_000);
        assert_eq!(ok(&mut c, "trash-restore --all", 0).lines().count(), 2);
        assert_eq!(ok(&mut c, "trash-list", 0), "");
    }

    #[test]
    fn a_directory_goes_whole_and_empty_really_empties() {
        let mut c = machine();
        ok(&mut c, "trash /home/user/notes", 0);
        assert_eq!(
            ok(
                &mut c,
                "cat /home/user/.local/share/Trash/files/notes/todo.md",
                0
            ),
            "todo\n"
        );
        ok(&mut c, "trash /home/user/report.txt", 0);
        assert_eq!(ok(&mut c, "trash-list", 0).lines().count(), 2);
        assert_eq!(ok(&mut c, "trash-empty", 0), "emptied 2 items\n");
        assert_eq!(ok(&mut c, "trash-list", 0), "");
        assert_eq!(ok(&mut c, "ls /home/user/.local/share/Trash/files", 0), "");
        assert_eq!(ok(&mut c, "ls /home/user/.local/share/Trash/info", 0), "");
    }

    #[test]
    fn the_trash_refuses_what_it_cannot_do_rather_than_pretending() {
        let mut c = machine();
        assert_eq!(run(&mut c, "trash /home/user/nope", 0).exit_code, 1);
        assert_eq!(run(&mut c, "trash -f /home/user/nope", 0).exit_code, 0);
        assert_eq!(
            run(&mut c, "trash --bogus /home/user/report.txt", 0).exit_code,
            2
        );
        assert_eq!(run(&mut c, "trash-restore missing.txt", 0).exit_code, 1);
        assert_eq!(run(&mut c, "trash-empty 30", 0).exit_code, 2);
        // The trash cannot be put in the trash.
        ok(&mut c, "trash /home/user/report.txt", 0);
        let r = run(
            &mut c,
            "trash /home/user/.local/share/Trash/files/report.txt",
            0,
        );
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("already in the trash"), "{}", r.stderr);
        // A restore over something that came back is refused unless forced.
        ok(&mut c, "echo new > /home/user/report.txt", 0);
        let r = run(&mut c, "trash-restore /home/user/report.txt", 0);
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("already exists"), "{}", r.stderr);
        ok(&mut c, "trash-restore -f /home/user/report.txt", 0);
        assert_eq!(ok(&mut c, "cat /home/user/report.txt", 0), "report\n");
    }

    #[test]
    fn gio_trash_is_the_same_trash() {
        let mut c = machine();
        ok(&mut c, "gio trash /home/user/report.txt", 0);
        assert_eq!(
            ok(&mut c, "gio trash --list", 0),
            "2026-09-17 09:00:00 /home/user/report.txt\n"
        );
        ok(&mut c, "gio trash --restore /home/user/report.txt", 0);
        assert_eq!(ok(&mut c, "cat /home/user/report.txt", 0), "report\n");
        // The opener still owns every other `gio` subcommand: `gio open` reaches it
        // (and answers for itself about applications), `gio mount` is refused by it.
        let opened = run(&mut c, "gio open /home/user/report.txt", 0);
        assert!(
            !opened.stderr.contains("unsupported subcommand"),
            "the trash swallowed `gio open`: {}",
            opened.stderr
        );
        let mounted = run(&mut c, "gio mount x", 0);
        assert_eq!(mounted.exit_code, 2);
        assert!(mounted.stderr.contains("gio open"), "{}", mounted.stderr);
    }

    #[test]
    fn a_path_with_spaces_survives_the_record() {
        let mut c = machine();
        ok(&mut c, "mkdir -p '/home/user/my docs'", 0);
        ok(&mut c, "echo hi > '/home/user/my docs/a b.txt'", 0);
        ok(&mut c, "trash '/home/user/my docs/a b.txt'", 0);
        let record = ok(
            &mut c,
            "cat '/home/user/.local/share/Trash/info/a b.txt.trashinfo'",
            0,
        );
        assert!(
            record.contains("Path=/home/user/my%20docs/a%20b.txt"),
            "{record}"
        );
        ok(&mut c, "trash-restore '/home/user/my docs/a b.txt'", 0);
        assert_eq!(ok(&mut c, "cat '/home/user/my docs/a b.txt'", 0), "hi\n");
    }
}
