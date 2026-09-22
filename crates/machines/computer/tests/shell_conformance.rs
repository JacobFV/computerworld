//! Executes every row of `docs/shell.md` so the published matrix cannot drift.
//! Each unsupported flag is asserted to fail loudly: a silently ignored flag is the
//! failure mode this suite exists to prevent.
use cw_computer::{shell, CommandResult, Computer, OfflineHost};

fn machine() -> Computer {
    let mut c = Computer::new("box", "user", "linux", true);
    for setup in [
        "mkdir -p /home/user/proj/sub",
        "printf 'alpha\\nbeta\\ngamma\\n' > /home/user/proj/a.txt",
        "echo hi > /home/user/proj/sub/b.txt",
        "echo dot > /home/user/.hidden",
        "ln -s /home/user/proj/a.txt /home/user/link",
    ] {
        let r = run(&mut c, setup);
        assert_eq!(r.exit_code, 0, "{setup}: {}", r.stderr);
    }
    c
}
fn run(c: &mut Computer, line: &str) -> CommandResult {
    shell::execute(c, line, 0, &mut OfflineHost)
}
/// stdout of a command the matrix says must succeed.
fn ok(c: &mut Computer, line: &str) -> String {
    let r = run(c, line);
    assert_eq!(r.exit_code, 0, "`{line}` should succeed: {}", r.stderr);
    r.stdout
}
/// A command that must fail with a particular status and name the reason.
fn refused_with(c: &mut Computer, line: &str, code: i32, mention: &str) {
    let r = run(c, line);
    assert_eq!(r.exit_code, code, "`{line}`: {:?}", r.stderr);
    assert!(
        r.stderr.contains(mention),
        "`{line}` should name `{mention}`, said {:?}",
        r.stderr
    );
}
/// An unsupported flag must be refused with status 2 and name itself. A short flag may
/// be named the way GNU names it (`invalid option -- 'Q'`), which is what this shell says.
fn refused(c: &mut Computer, line: &str, mention: &str) {
    let r = run(c, line);
    assert_eq!(r.exit_code, 2, "`{line}` must be refused, not ignored");
    let gnu = mention
        .strip_prefix('-')
        .filter(|rest| rest.len() == 1)
        .map(|rest| format!("'{rest}'"));
    assert!(
        r.stderr.contains(mention) || gnu.is_some_and(|g| r.stderr.contains(&g)),
        "`{line}` should name `{mention}`, said {:?}",
        r.stderr
    );
}

#[test]
fn exit_codes_carry_the_classification() {
    let mut c = machine();
    assert_eq!(run(&mut c, "true").exit_code, 0);
    assert_eq!(run(&mut c, "false").exit_code, 1);
    assert_eq!(run(&mut c, "test -f /nope").exit_code, 1);
    assert_eq!(run(&mut c, "cat /nope").exit_code, 1);
    assert_eq!(run(&mut c, "grep zzz /home/user/proj/a.txt").exit_code, 1);
    // 2 is reserved for "this world does not implement that".
    assert_eq!(run(&mut c, "ls --bogus").exit_code, 2);
    assert_eq!(run(&mut c, "echo hi &&").exit_code, 2);
    assert_eq!(run(&mut c, "echo 'unterminated").exit_code, 2);
    assert_eq!(run(&mut c, "echo hi >").exit_code, 2);
    // 127/126 separate "no such command" from "cannot run it".
    assert_eq!(run(&mut c, "nosuchcommand").exit_code, 127);
    assert_eq!(run(&mut c, "Write-Output x").exit_code, 127);
    let r = run(
        &mut c,
        "echo '#!/bin/perl' > /tmp/p; chmod 755 /tmp/p; /tmp/p",
    );
    assert_eq!(r.exit_code, 126, "{}", r.stderr);
    // A nested shell propagates its status instead of collapsing it.
    assert_eq!(run(&mut c, "sh -c 'nosuchcommand'").exit_code, 127);
    assert_eq!(run(&mut c, "sh -c 'ls --bogus'").exit_code, 2);
    // Its stderr reaches the caller even when it succeeds.
    let r = run(&mut c, "sh -c 'cat /nope; true'");
    assert_eq!(r.exit_code, 0);
    assert!(
        r.stderr.contains("nope"),
        "a script must not lose stderr: {r:?}"
    );
}

#[test]
fn redirection_covers_both_descriptors() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "echo one > /tmp/r; cat /tmp/r"), "one\n");
    assert_eq!(ok(&mut c, "echo two >> /tmp/r; cat /tmp/r"), "one\ntwo\n");
    // 2>/dev/null discards and creates nothing.
    let r = run(&mut c, "cat /nope 2>/dev/null");
    assert_eq!(
        (r.exit_code, r.stdout.as_str(), r.stderr.as_str()),
        (1, "", "")
    );
    assert_eq!(run(&mut c, "test -e /dev/null").exit_code, 1);
    // 2> and 2>> capture stderr to a file.
    run(&mut c, "cat /nope 2>/tmp/e");
    run(&mut c, "cat /nope 2>>/tmp/e");
    assert_eq!(ok(&mut c, "wc -l /tmp/e"), "2 /tmp/e\n");
    // 2>&1 folds stderr into stdout; >&2 folds the other way.
    let r = run(&mut c, "cat /nope 2>&1");
    assert!(!r.stdout.is_empty() && r.stderr.is_empty(), "{r:?}");
    let r = run(&mut c, "echo loud >&2");
    assert!(r.stdout.is_empty() && r.stderr.contains("loud"), "{r:?}");
    // Order matters: `>f 2>&1` sends both to the file, `2>&1 >f` leaves stderr behind.
    run(&mut c, "cat /nope > /tmp/both 2>&1");
    assert!(ok(&mut c, "cat /tmp/both").contains("nope"));
    let r = run(&mut c, "cat /nope 2>&1 > /tmp/split");
    assert!(!r.stdout.is_empty(), "stderr should still reach the caller");
    assert_eq!(ok(&mut c, "cat /tmp/split"), "");
    // &> takes both descriptors.
    run(&mut c, "cat /nope &> /tmp/all");
    assert!(ok(&mut c, "cat /tmp/all").contains("nope"));
    run(&mut c, "cat /nope &>> /tmp/all");
    assert_eq!(ok(&mut c, "wc -l /tmp/all"), "2 /tmp/all\n");
    assert_eq!(ok(&mut c, "echo one 1> /tmp/one; cat /tmp/one"), "one\n");
    assert_eq!(
        ok(&mut c, "echo two 1>> /tmp/one; cat /tmp/one"),
        "one\ntwo\n"
    );
    // A digit only prefixes an operator when it is glued on.
    assert_eq!(ok(&mut c, "echo 2 > /tmp/two; cat /tmp/two"), "2\n");
}

#[test]
fn clear_is_a_screen_action_not_output() {
    let mut c = machine();
    let r = run(&mut c, "clear");
    assert_eq!((r.exit_code, r.stdout.as_str(), r.clear), (0, "", true));
    assert!(!run(&mut c, "echo hi").clear, "only clear raises the flag");
    assert!(
        run(&mut c, "echo hi; clear").clear,
        "the flag survives a list"
    );
    refused(&mut c, "clear extra", "clear");
}

#[test]
fn date_is_a_clock_not_a_tick() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "date"), "Thu Sep 17 09:00:00 UTC 2026\n");
    assert_eq!(ok(&mut c, "date +%F"), "2026-09-17\n");
    assert_eq!(
        ok(&mut c, "date '+%Y-%m-%dT%H:%M:%SZ'"),
        "2026-09-17T09:00:00Z\n"
    );
    assert_eq!(ok(&mut c, "date +%s"), "1789635600\n");
    assert_eq!(
        ok(&mut c, "date -u '+%a %b %e %T %Z'"),
        "Thu Sep 17 09:00:00 UTC\n"
    );
    // The clock advances with simulated time, and only with it.
    let later = shell::execute(&mut c, "date +%T", 3_600_000_000, &mut OfflineHost);
    assert_eq!(later.stdout, "10:00:00\n");
    let tomorrow = shell::execute(&mut c, "date +%F", 86_400_000_000, &mut OfflineHost);
    assert_eq!(tomorrow.stdout, "2026-09-18\n");
    refused(&mut c, "date -d yesterday", "-d");
    refused(&mut c, "date +%Q", "%Q");
}

#[test]
fn ls_honours_its_flag_combinations() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "ls /home/user"), "link\nproj\n");
    assert_eq!(
        ok(&mut c, "ls -a /home/user"),
        ".\n..\n.hidden\nlink\nproj\n"
    );
    assert_eq!(ok(&mut c, "ls -A /home/user"), ".hidden\nlink\nproj\n");
    assert_eq!(ok(&mut c, "ls -r /home/user"), "proj\nlink\n");
    assert_eq!(ok(&mut c, "ls -d /home/user/proj"), "/home/user/proj\n");
    assert_eq!(ok(&mut c, "ls -F /home/user"), "link@\nproj/\n");
    let long = ok(&mut c, "ls -la /home/user/proj");
    assert!(long.starts_with("total "), "{long}");
    assert!(long.contains("-rw-r--r--"), "{long}");
    assert!(long.contains("drwxr-xr-x"), "{long}");
    assert!(long.contains("Sep 17 09:00"), "{long}");
    assert!(long.lines().any(|l| l.ends_with(" a.txt")), "{long}");
    assert!(
        long.lines().any(|l| l.ends_with(" .")),
        "-a must list . and .."
    );
    assert!(ok(&mut c, "ls -l /home/user/proj").contains(" 17 "));
    // A symlink names its target in long form.
    assert!(ok(&mut c, "ls -l /home/user/link").contains("-> /home/user/proj/a.txt"));
    // -R descends and titles each directory.
    let recursive = ok(&mut c, "ls -R /home/user/proj");
    assert!(recursive.contains("/home/user/proj:"), "{recursive}");
    assert!(recursive.contains("/home/user/proj/sub:"), "{recursive}");
    assert_eq!(run(&mut c, "ls /nope").exit_code, 1);
    // -1 is the default shape; -p marks only directories; -i prefixes the inode.
    assert_eq!(ok(&mut c, "ls -1 /home/user"), "link\nproj\n");
    assert_eq!(ok(&mut c, "ls -p /home/user"), "link\nproj/\n");
    let inode = ok(&mut c, "ls -i /home/user/proj");
    assert!(
        inode
            .lines()
            .all(|l| l.split(' ').next().unwrap().parse::<u64>().is_ok()),
        "{inode}"
    );
    assert_eq!(ok(&mut c, "ls --color=never /home/user"), "link\nproj\n");
    // The long form carries owner and group as separate columns.
    let long = ok(&mut c, "ls -l /home/user/proj");
    assert!(long.contains(" user user "), "{long}");
    assert!(ok(&mut c, "ls -ln /home/user/proj").contains(" 1000 1000 "));
    // --json answers "what is every entry?" in one call, instead of an N+1 test -d storm.
    let json = ok(&mut c, "ls --json /home/user");
    assert!(
        json.starts_with('[') && json.trim_end().ends_with(']'),
        "{json}"
    );
    assert!(json.contains("\"name\":\"proj\""), "{json}");
    assert!(json.contains("\"kind\":\"directory\""), "{json}");
    assert!(json.contains("\"kind\":\"symlink\""), "{json}");
    assert!(
        json.contains("\"target\":\"/home/user/proj/a.txt\""),
        "{json}"
    );
    assert!(json.contains("\"mode\":\"0755\""), "{json}");
    assert!(json.contains("\"mtime\":\"2026-09-17 09:00:00\""), "{json}");
    refused(&mut c, "ls -Q /home/user", "-Q");
    refused(&mut c, "ls --color=always /home/user", "--color");
    // `--color` and `--json` have no short spelling, and `-c`/`-u` stay refused
    // rather than quietly meaning something else.
    refused(&mut c, "ls -c /home/user", "-c");
    refused(&mut c, "ls -u /home/user", "-u");
}

/// The consumer asked for globbing that actually reaches the commands that take files.
#[test]
fn globbing_reaches_every_command_that_takes_a_path() {
    let mut c = machine();
    ok(&mut c, "mkdir -p /tmp/g/keep");
    for name in ["a.txt", "b.txt", "c.log", "d1.log", "d2.log"] {
        ok(&mut c, &format!("echo {name} > /tmp/g/{name}"));
    }
    assert_eq!(
        ok(&mut c, "ls /tmp/g/*.txt"),
        "/tmp/g/a.txt\n/tmp/g/b.txt\n"
    );
    // `~` and a glob in the same word.
    ok(
        &mut c,
        "mkdir -p ~/t; echo x > ~/t/one.txt; echo y > ~/t/two.txt",
    );
    assert_eq!(
        ok(&mut c, "ls ~/t/*.txt"),
        "/home/user/t/one.txt\n/home/user/t/two.txt\n"
    );
    // A class and a single-character wildcard.
    assert_eq!(
        ok(&mut c, "ls /tmp/g/d[12].log"),
        "/tmp/g/d1.log\n/tmp/g/d2.log\n"
    );
    assert_eq!(ok(&mut c, "ls /tmp/g/d[!1].log"), "/tmp/g/d2.log\n");
    assert_eq!(
        ok(&mut c, "ls /tmp/g/?.txt"),
        "/tmp/g/a.txt\n/tmp/g/b.txt\n"
    );
    assert_eq!(
        ok(&mut c, "ls /tmp/g/[a-b].txt"),
        "/tmp/g/a.txt\n/tmp/g/b.txt\n"
    );
    // Brace expansion, including a numeric range, and nesting.
    assert_eq!(ok(&mut c, "echo pre{a,b}post"), "preapost prebpost\n");
    assert_eq!(ok(&mut c, "echo {1..4}"), "1 2 3 4\n");
    assert_eq!(ok(&mut c, "echo {a,b}{1,2}"), "a1 a2 b1 b2\n");
    ok(&mut c, "mkdir -p /tmp/g/{x,y}/deep");
    assert_eq!(run(&mut c, "test -d /tmp/g/y/deep").exit_code, 0);
    // The glob reaches cp and rm, not just ls.
    ok(&mut c, "mkdir -p /tmp/logs");
    ok(&mut c, "cp /tmp/g/*.log /tmp/logs/");
    assert_eq!(ok(&mut c, "ls /tmp/logs"), "c.log\nd1.log\nd2.log\n");
    ok(&mut c, "mkdir -p /tmp/g/dirA /tmp/g/dirB");
    ok(&mut c, "rm -r /tmp/g/dir*/");
    assert_eq!(run(&mut c, "test -d /tmp/g/dirA").exit_code, 1);
    // No match is the literal word, as bash behaves without nullglob.
    assert_eq!(ok(&mut c, "echo /tmp/g/*.nope"), "/tmp/g/*.nope\n");
    assert_eq!(run(&mut c, "ls /tmp/g/*.nope").exit_code, 1);
    // A pattern that arrives from a variable stays data.
    assert_eq!(ok(&mut c, "P='*.txt'; echo $P"), "*.txt\n");
    // A dot file is not matched unless the pattern says so.
    ok(&mut c, "echo h > /tmp/g/.hide");
    let starred = ok(&mut c, "ls /tmp/g/*");
    assert!(
        starred.starts_with("/tmp/g/a.txt\n/tmp/g/b.txt\n"),
        "{starred}"
    );
    assert!(!starred.contains(".hide"), "{starred}");
    assert!(starred.contains("/tmp/g/keep:\n"), "{starred}");
    assert_eq!(ok(&mut c, "ls -d /tmp/g/.*"), "/tmp/g/.hide\n");
}

/// Permissions have to bite: a mode the world stores is a mode a read obeys.
#[test]
fn permissions_are_enforced_and_ownership_is_real() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "umask"), "0022\n");
    ok(&mut c, "umask 077");
    ok(&mut c, "echo secret > /tmp/private");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/private"), "600\n");
    ok(&mut c, "mkdir /tmp/privdir");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/privdir"), "700\n");
    assert_eq!(ok(&mut c, "umask -S"), "u=rwx,g=,o=\n");
    ok(&mut c, "umask 022");
    // A file another user cannot read fails to read, with the real message.
    ok(&mut c, "echo mine > /tmp/mine; chmod 600 /tmp/mine");
    let r = run(&mut c, "sudo -u other cat /tmp/mine");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Permission denied"), "{}", r.stderr);
    // root is not stopped by a mode.
    assert_eq!(ok(&mut c, "sudo cat /tmp/mine"), "mine\n");
    // chmod is observable through stat and ls alike.
    ok(&mut c, "chmod u+x,go=r /tmp/mine");
    assert_eq!(ok(&mut c, "stat -c %A /tmp/mine"), "-rwxr--r--\n");
    // chown needs root; chgrp is the owner's to give.
    assert_eq!(run(&mut c, "chown other /tmp/mine").exit_code, 1);
    ok(&mut c, "sudo chown other:staff /tmp/mine");
    assert_eq!(ok(&mut c, "stat -c '%U %G' /tmp/mine"), "other staff\n");
    ok(&mut c, "sudo chown -R user:user /tmp/privdir");
    assert_eq!(ok(&mut c, "stat -c '%U %G' /tmp/privdir"), "user user\n");
    ok(&mut c, "echo g > /tmp/privdir/inner");
    ok(&mut c, "chgrp users /tmp/privdir/inner");
    assert_eq!(ok(&mut c, "stat -c %G /tmp/privdir/inner"), "users\n");
    // A directory without the execute bit cannot be walked into.
    ok(&mut c, "sudo mkdir /tmp/closed; sudo chmod 700 /tmp/closed");
    ok(&mut c, "sudo sh -c 'echo inside > /tmp/closed/f'");
    assert_eq!(run(&mut c, "cat /tmp/closed/f").exit_code, 1);
    refused(
        &mut c,
        "chmod --reference=/tmp/mine /tmp/private",
        "--reference",
    );
    // A flag that would be a silent no-op is refused, not accepted and ignored.
    refused(&mut c, "chown -c user /tmp/mine", "-c");
    refused(&mut c, "rm -I /tmp/mine", "-I");
    refused(&mut c, "install -C /tmp/mine /tmp/mine2", "-C");
}

#[test]
fn stat_reports_the_vfs_and_formats_it() {
    let mut c = machine();
    let block = ok(&mut c, "stat /home/user/proj/a.txt");
    for fragment in [
        "  File: /home/user/proj/a.txt",
        "Size: 17",
        "regular file",
        "(0644/-rw-r--r--)",
        "Uid: ( 1000/    user)",
        "Modify: 2026-09-17 09:00:00",
    ] {
        assert!(block.contains(fragment), "missing {fragment:?} in {block}");
    }
    assert_eq!(
        ok(&mut c, "stat -c '%n %s %F %a %U %h' /home/user/proj/a.txt"),
        "/home/user/proj/a.txt 17 regular file 644 user 1\n"
    );
    assert_eq!(
        ok(&mut c, "stat --format=%F /home/user/proj"),
        "directory\n"
    );
    assert_eq!(ok(&mut c, "stat -c %s /home/user/proj"), "4096\n");
    assert_eq!(ok(&mut c, "stat -c %F /home/user/link"), "symbolic link\n");
    assert_eq!(ok(&mut c, "stat -Lc %F /home/user/link"), "regular file\n");
    assert_eq!(
        ok(&mut c, "stat -c %Y /home/user/proj/a.txt"),
        "1789635600\n"
    );
    assert_eq!(run(&mut c, "stat /nope").exit_code, 1);
    // Access, modification and change are three separate fields now.
    ok(
        &mut c,
        "touch -a -d 2026-09-18T10:00:00 /home/user/proj/a.txt",
    );
    assert_eq!(
        ok(&mut c, "stat -c '%X %Y' /home/user/proj/a.txt"),
        "1789725600 1789635600\n"
    );
    // The group is its own column, and %N spells a symlink out.
    assert_eq!(ok(&mut c, "stat -c %G /home/user/proj/a.txt"), "user\n");
    assert_eq!(
        ok(&mut c, "stat -c %N /home/user/link"),
        "'/home/user/link' -> '/home/user/proj/a.txt'\n"
    );
    // -f describes the filesystem, not the file.
    let fs = ok(&mut c, "stat -f /home/user");
    assert!(fs.contains("Block size: 4096"), "{fs}");
    assert!(fs.contains("Namelen: 255"), "{fs}");
    assert_eq!(ok(&mut c, "stat -f -c %T /home/user"), "ext2/ext3\n");
    refused(&mut c, "stat -c %Q /home/user", "%Q");
    refused(&mut c, "stat -f -c %Q /home/user", "%Q");
    refused(&mut c, "stat", "missing operand");
}

#[test]
fn grep_works_without_recursion_and_counts() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "grep beta /home/user/proj/a.txt"), "beta\n");
    assert_eq!(ok(&mut c, "grep -c a /home/user/proj/a.txt"), "3\n");
    assert_eq!(ok(&mut c, "grep -n beta /home/user/proj/a.txt"), "2:beta\n");
    assert_eq!(ok(&mut c, "grep -i BETA /home/user/proj/a.txt"), "beta\n");
    assert_eq!(
        ok(&mut c, "grep -v beta /home/user/proj/a.txt"),
        "alpha\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, "grep -l beta /home/user/proj/a.txt"),
        "/home/user/proj/a.txt\n"
    );
    assert_eq!(ok(&mut c, "cat /home/user/proj/a.txt | grep -c a"), "3\n");
    assert_eq!(
        ok(&mut c, "grep -rl hi /home/user/proj"),
        "/home/user/proj/sub/b.txt\n"
    );
    assert!(ok(&mut c, "grep -rn alpha /home/user/proj").contains("a.txt:1:alpha"));
    assert_eq!(ok(&mut c, "grep -q beta /home/user/proj/a.txt"), "");
    // A miss is status 1, and -c still prints the zero it counted.
    let r = run(&mut c, "grep -c zzz /home/user/proj/a.txt");
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, "0\n"));
    assert_eq!(run(&mut c, "grep zzz /home/user/proj/a.txt").exit_code, 1);
    // Two files earn filename prefixes; -h suppresses them.
    let two = ok(&mut c, "grep a /home/user/proj/a.txt /home/user/proj/a.txt");
    assert!(
        two.lines().all(|l| l.starts_with("/home/user/proj/a.txt:")),
        "{two}"
    );
    assert!(!ok(
        &mut c,
        "grep -h a /home/user/proj/a.txt /home/user/proj/a.txt"
    )
    .contains(':'));
    refused(&mut c, "grep -A x beta /home/user/proj/a.txt", "-A");
    refused(&mut c, "grep --include=x beta /home/user/proj", "--include");
}

#[test]
fn sed_selects_lines_as_well_as_substituting() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "sed -n 2p /home/user/proj/a.txt"), "beta\n");
    assert_eq!(
        ok(&mut c, "sed -n '1,2p' /home/user/proj/a.txt"),
        "alpha\nbeta\n"
    );
    assert_eq!(ok(&mut c, "sed -n '$p' /home/user/proj/a.txt"), "gamma\n");
    assert_eq!(
        ok(&mut c, "sed -n '2,$p' /home/user/proj/a.txt"),
        "beta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, "sed '2d' /home/user/proj/a.txt"),
        "alpha\ngamma\n"
    );
    // Without -n sed auto-prints, so `p` duplicates the selected line.
    assert_eq!(
        ok(&mut c, "sed 2p /home/user/proj/a.txt"),
        "alpha\nbeta\nbeta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, "cat /home/user/proj/a.txt | sed -n 3p"),
        "gamma\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's/beta/BETA/' /home/user/proj/a.txt"),
        "alpha\nBETA\ngamma\n"
    );
    ok(
        &mut c,
        "cp /home/user/proj/a.txt /tmp/edit; sed -i 's/alpha/ALPHA/' /tmp/edit",
    );
    assert_eq!(ok(&mut c, "sed -n 1p /tmp/edit"), "ALPHA\n");
    refused(
        &mut c,
        "sed 'Z' /home/user/proj/a.txt",
        "unknown command `Z`",
    );
    assert_eq!(
        ok(&mut c, "sed -r 's/(al)pha/\\1/' /home/user/proj/a.txt"),
        "al\nbeta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, "sed -n -e 1p -e 2p /home/user/proj/a.txt"),
        "alpha\nbeta\n"
    );
}

#[test]
fn disk_and_hardware_probes_answer() {
    let mut c = machine();
    // du is modelled over the VFS: adding bytes moves the number.
    let total = |c: &mut Computer| -> u64 {
        ok(c, "du -s /home/user/proj")
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let before = total(&mut c);
    ok(
        &mut c,
        "head -n 2 /home/user/proj/a.txt > /home/user/proj/big",
    );
    assert!(total(&mut c) > before, "du must reflect the VFS");
    assert!(ok(&mut c, "du -sh /home/user/proj").ends_with("\t/home/user/proj\n"));
    assert!(ok(&mut c, "du -a /home/user/proj").contains("/home/user/proj/a.txt"));
    assert!(
        !ok(&mut c, "du /home/user/proj").contains("a.txt"),
        "files need -a"
    );
    refused(&mut c, "du -x /home/user", "invalid option -- 'x'");
    // df: fixed capacity, modelled usage, one filesystem.
    let df = ok(&mut c, "df");
    assert!(df.starts_with("Filesystem"), "{df}");
    assert!(df.contains("/dev/vda1"), "{df}");
    assert!(df.trim_end().ends_with(" /"), "{df}");
    assert_eq!(df.lines().count(), 2, "one modelled filesystem");
    assert!(ok(&mut c, "df -h").contains("64G"));
    assert!(ok(&mut c, "df -hT").contains("ext4"));
    assert_eq!(run(&mut c, "df /nope").exit_code, 1);
    refused(&mut c, "df -i", "invalid option -- 'i'");
    // Fixed facts are constant across calls.
    assert_eq!(ok(&mut c, "nproc"), "4\n");
    let all = ok(&mut c, "nproc --all");
    assert_eq!(all, ok(&mut c, "nproc"));
    refused(&mut c, "nproc --ignore=1", "--ignore");
    assert_eq!(ok(&mut c, "uptime -s"), "2026-09-17 09:00:00\n");
    assert_eq!(ok(&mut c, "uptime -p"), "up 0 minutes\n");
    assert!(ok(&mut c, "uptime").contains("load average: 0.00, 0.00, 0.00"));
    let hour = shell::execute(&mut c, "uptime -p", 3_600_000_000, &mut OfflineHost);
    assert_eq!(hour.stdout, "up 1 hour, 0 minutes\n");
    refused(&mut c, "uptime -h", "invalid option -- 'h'");
}

#[test]
fn network_and_path_probes_answer() {
    let mut c = machine();
    let addr = ok(&mut c, "ip addr");
    assert!(addr.contains("inet 10.0.2.15/24"), "{addr}");
    assert!(addr.contains("link/ether 52:54:00:12:34:56"), "{addr}");
    assert!(addr.contains("inet 127.0.0.1/8"), "{addr}");
    assert_eq!(ok(&mut c, "ip a"), addr);
    assert_eq!(ok(&mut c, "ip addr show"), addr);
    assert!(ok(&mut c, "ip route").contains("default via 10.0.2.1 dev eth0"));
    assert!(ok(&mut c, "ip link").contains("eth0"));
    refused(&mut c, "ip netns", "netns");
    refused(&mut c, "ip addr add 1.2.3.4 dev eth0", "unsupported action");
    refused(&mut c, "ip", "missing object");
    // which resolves builtins nominally and installed files really.
    assert_eq!(ok(&mut c, "which grep"), "/usr/bin/grep\n");
    let r = run(&mut c, "which definitely-not-here");
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    ok(
        &mut c,
        "mkdir -p /tmp/bin; echo '#!/bin/sh' > /tmp/bin/tool",
    );
    ok(&mut c, "export PATH=/tmp/bin:/bin:/usr/bin");
    assert_eq!(ok(&mut c, "which tool"), "/tmp/bin/tool\n");
    refused(&mut c, "which -s grep", "invalid option -- 's'");
}

#[test]
fn sudo_runs_as_another_identity() {
    let mut c = machine();
    c.vfs.write("/root-only", b"secret", "root", 0).unwrap();
    c.vfs.chmod("/root-only", 0o600).unwrap();
    assert_eq!(run(&mut c, "cat /root-only").exit_code, 1);
    assert_eq!(ok(&mut c, "sudo cat /root-only"), "secret");
    assert_eq!(ok(&mut c, "sudo whoami"), "root\n");
    assert_eq!(ok(&mut c, "sudo -u user whoami"), "user\n");
    assert_eq!(ok(&mut c, "whoami"), "user\n", "the swap must not leak");
    assert_eq!(ok(&mut c, "sudo -v"), "");
    refused(&mut c, "sudo", "missing command");
    refused(&mut c, "sudo -Z ls", "-Z");
    assert_eq!(run(&mut c, "sudo nosuchcommand").exit_code, 127);
}

#[test]
fn text_utilities_refuse_what_they_cannot_do() {
    let mut c = machine();
    assert_eq!(
        ok(&mut c, "wc -l /home/user/proj/a.txt"),
        "3 /home/user/proj/a.txt\n"
    );
    assert_eq!(
        ok(&mut c, "wc -w /home/user/proj/a.txt"),
        "3 /home/user/proj/a.txt\n"
    );
    assert_eq!(
        ok(&mut c, "wc /home/user/proj/a.txt"),
        "3 3 17 /home/user/proj/a.txt\n"
    );
    assert_eq!(ok(&mut c, "head -n 1 /home/user/proj/a.txt"), "alpha\n");
    assert_eq!(ok(&mut c, "tail -n 1 /home/user/proj/a.txt"), "gamma\n");
    assert_eq!(ok(&mut c, "printf '2\\n10\\n1\\n' | sort"), "1\n10\n2\n");
    assert_eq!(ok(&mut c, "printf '2\\n10\\n1\\n' | sort -n"), "1\n2\n10\n");
    assert_eq!(
        ok(&mut c, "printf 'a\\na\\nb\\n' | uniq -c"),
        "      2 a\n      1 b\n"
    );
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | sort -u"), "a\nb\n");
    assert_eq!(ok(&mut c, "head -c 3 /home/user/proj/a.txt"), "alp");
    assert_eq!(ok(&mut c, "printf 'b 1\\na 2\\n' | sort -k2"), "b 1\na 2\n");
    assert_eq!(
        ok(&mut c, "wc -L /home/user/proj/a.txt"),
        "5 /home/user/proj/a.txt\n"
    );
    assert_eq!(ok(&mut c, "printf 'A\\na\\n' | uniq -i"), "A\n");
    // File commands parse their flags rather than skipping anything dash-shaped.
    assert_eq!(run(&mut c, "mkdir /home/user/proj").exit_code, 1);
    assert_eq!(run(&mut c, "mkdir /tmp/a/b/c").exit_code, 1);
    assert_eq!(ok(&mut c, "mkdir -p /tmp/a/b/c; ls /tmp/a/b"), "c\n");
    ok(&mut c, "mkdir -m 700 /tmp/locked");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/locked"), "700\n");
    refused(&mut c, "touch -t 1 /tmp/x", "-t");
    refused(&mut c, "touch --time=access /tmp/x", "--time");
    assert_eq!(run(&mut c, "cp /home/user/proj /tmp/copy").exit_code, 1);
    ok(&mut c, "cp -r /home/user/proj /tmp/copy");
    assert_eq!(ok(&mut c, "cat /tmp/copy/sub/b.txt"), "hi\n");
    refused(&mut c, "cp --bogus a b", "--bogus");
    refused(&mut c, "rm --bogus a", "--bogus");
}

/// The consumer's own report asked for these by name: the copy/move/remove flag set,
/// the classifier, and a listing that says what every entry *is* in one call.
#[test]
fn copy_move_remove_follow_coreutils() {
    let mut c = machine();
    ok(&mut c, "mkdir -p /tmp/d");
    ok(&mut c, "echo one > /tmp/one.txt");
    // Copying into an existing directory keeps the name.
    ok(&mut c, "cp /tmp/one.txt /tmp/d");
    assert_eq!(ok(&mut c, "cat /tmp/d/one.txt"), "one\n");
    // -n and -i both decline an existing destination; -f replaces it.
    ok(&mut c, "echo two > /tmp/two.txt");
    ok(&mut c, "cp -n /tmp/two.txt /tmp/d/one.txt");
    assert_eq!(ok(&mut c, "cat /tmp/d/one.txt"), "one\n");
    ok(&mut c, "cp -i /tmp/two.txt /tmp/d/one.txt");
    assert_eq!(ok(&mut c, "cat /tmp/d/one.txt"), "one\n");
    ok(&mut c, "cp -f /tmp/two.txt /tmp/d/one.txt");
    assert_eq!(ok(&mut c, "cat /tmp/d/one.txt"), "two\n");
    // -v names both sides; -t puts the directory first; -T forbids the directory rule.
    assert_eq!(
        ok(&mut c, "cp -v /tmp/one.txt /tmp/copy1.txt"),
        "'/tmp/one.txt' -> '/tmp/copy1.txt'\n"
    );
    ok(&mut c, "cp -t /tmp/d /tmp/one.txt /tmp/two.txt");
    assert_eq!(ok(&mut c, "cat /tmp/d/two.txt"), "two\n");
    ok(&mut c, "mkdir -p /tmp/e");
    ok(&mut c, "cp -rT /tmp/d /tmp/e");
    assert_eq!(ok(&mut c, "cat /tmp/e/two.txt"), "two\n");
    // Without -p a new copy takes the source's permissions through the umask.
    ok(
        &mut c,
        "chmod 700 /tmp/one.txt; cp /tmp/one.txt /tmp/copy700.txt",
    );
    assert_eq!(ok(&mut c, "stat -c %a /tmp/copy700.txt"), "700\n");
    ok(
        &mut c,
        "chmod 777 /tmp/one.txt; cp /tmp/one.txt /tmp/copy777.txt",
    );
    assert_eq!(ok(&mut c, "stat -c %a /tmp/copy777.txt"), "755\n");
    // -p carries the mode and the timestamps.
    ok(&mut c, "chmod 641 /tmp/one.txt");
    ok(&mut c, "touch -d 2026-09-19T08:00:00 /tmp/one.txt");
    ok(&mut c, "cp -p /tmp/one.txt /tmp/kept.txt");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/kept.txt"), "641\n");
    assert_eq!(
        ok(&mut c, "stat -c %y /tmp/kept.txt"),
        "2026-09-19 08:00:00.000000000 +0000\n"
    );
    // A directory without -r is refused with the real message.
    let r = run(&mut c, "cp /tmp/d /tmp/f");
    assert_eq!(r.exit_code, 1);
    assert!(
        r.stderr.contains("-r not specified; omitting directory"),
        "{}",
        r.stderr
    );
    // -a keeps a symlink a symlink.
    ok(&mut c, "ln -s /tmp/one.txt /tmp/d/alias");
    ok(&mut c, "cp -a /tmp/d /tmp/archive");
    assert_eq!(
        ok(&mut c, "stat -c %F /tmp/archive/alias"),
        "symbolic link\n"
    );

    // mv into a directory, across directories, and the overwrite rules.
    ok(&mut c, "mv /tmp/copy1.txt /tmp/e");
    assert_eq!(ok(&mut c, "cat /tmp/e/copy1.txt"), "one\n");
    assert_eq!(
        ok(&mut c, "mv -v /tmp/e/copy1.txt /tmp/e/renamed.txt"),
        "renamed '/tmp/e/copy1.txt' -> '/tmp/e/renamed.txt'\n"
    );
    ok(&mut c, "mkdir -p /tmp/target");
    let r = run(&mut c, "mv /tmp/e/renamed.txt /tmp/target");
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    ok(&mut c, "echo x > /tmp/plain");
    let r = run(&mut c, "mv /tmp/plain /tmp/target");
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    ok(&mut c, "echo y > /tmp/other");
    let r = run(&mut c, "mv -T /tmp/other /tmp/target");
    assert_eq!(r.exit_code, 1);
    assert!(
        r.stderr.contains("cannot overwrite directory"),
        "{}",
        r.stderr
    );
    ok(&mut c, "mv -n /tmp/other /tmp/target/plain");
    assert_eq!(ok(&mut c, "cat /tmp/target/plain"), "x\n");

    // rm: a directory needs -r or -d, and -v says what went.
    let r = run(&mut c, "rm /tmp/target");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Is a directory"), "{}", r.stderr);
    ok(&mut c, "mkdir -p /tmp/empty");
    assert_eq!(ok(&mut c, "rm -dv /tmp/empty"), "removed '/tmp/empty'\n");
    let r = run(&mut c, "rm -d /tmp/target");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Directory not empty"), "{}", r.stderr);
    ok(&mut c, "rm -r /tmp/target");
    assert_eq!(run(&mut c, "test -e /tmp/target").exit_code, 1);
    // -i removes nothing, because a prompt at end of input answers no.
    ok(&mut c, "echo keep > /tmp/keep");
    ok(&mut c, "rm -i /tmp/keep");
    assert_eq!(ok(&mut c, "cat /tmp/keep"), "keep\n");
    // A missing operand is an error unless -f said to ignore it.
    assert_eq!(run(&mut c, "rm /tmp/nothing").exit_code, 1);
    assert_eq!(run(&mut c, "rm -f /tmp/nothing").exit_code, 0);

    // rmdir -p walks up the components the operand names, and no further.
    ok(&mut c, "mkdir -p /tmp/a1/b1/c1");
    ok(&mut c, "cd /tmp; rmdir -p a1/b1/c1");
    assert_eq!(run(&mut c, "test -d /tmp/a1").exit_code, 1);
    // A non-empty parent stops it, loudly, exactly as GNU rmdir does.
    ok(&mut c, "mkdir -p /tmp/a2/b2");
    ok(&mut c, "echo x > /tmp/a2/keep");
    let r = run(&mut c, "cd /tmp; rmdir -p a2/b2");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Directory not empty"), "{}", r.stderr);
    assert_eq!(run(&mut c, "test -d /tmp/a2/b2").exit_code, 1);
    assert_eq!(run(&mut c, "rmdir /tmp/e").exit_code, 1);
}

#[test]
fn links_are_real_links() {
    let mut c = machine();
    ok(&mut c, "mkdir -p /tmp/l/sub");
    ok(&mut c, "echo body > /tmp/l/file");
    // A hard link shares the inode and the bytes.
    ok(&mut c, "ln /tmp/l/file /tmp/l/hard");
    assert_eq!(
        ok(&mut c, "stat -c %i /tmp/l/file"),
        ok(&mut c, "stat -c %i /tmp/l/hard")
    );
    assert_eq!(ok(&mut c, "stat -c %h /tmp/l/file"), "2\n");
    // -s, -f and the directory rule.
    ok(&mut c, "ln -s /tmp/l/file /tmp/l/soft");
    assert_eq!(ok(&mut c, "readlink /tmp/l/soft"), "/tmp/l/file\n");
    assert_eq!(run(&mut c, "ln -s /tmp/l/file /tmp/l/soft").exit_code, 1);
    ok(&mut c, "ln -sf /tmp/l/hard /tmp/l/soft");
    assert_eq!(ok(&mut c, "readlink /tmp/l/soft"), "/tmp/l/hard\n");
    ok(&mut c, "ln -s /tmp/l/file /tmp/l/sub");
    assert_eq!(ok(&mut c, "readlink /tmp/l/sub/file"), "/tmp/l/file\n");
    // -r writes the target relative to the link's own directory.
    ok(&mut c, "ln -sr /tmp/l/file /tmp/l/sub/rel");
    assert_eq!(ok(&mut c, "readlink /tmp/l/sub/rel"), "../file\n");
    assert_eq!(ok(&mut c, "cat /tmp/l/sub/rel"), "body\n");
    // -n/-T replace a link to a directory rather than writing inside it.
    ok(&mut c, "ln -s /tmp/l/sub /tmp/l/subalias");
    ok(&mut c, "ln -sfn /tmp/l/hard /tmp/l/subalias");
    assert_eq!(ok(&mut c, "readlink /tmp/l/subalias"), "/tmp/l/hard\n");
    ok(&mut c, "ln -s /tmp/l/sub /tmp/l/alias2");
    ok(&mut c, "ln -sfT /tmp/l/file /tmp/l/alias2");
    assert_eq!(ok(&mut c, "readlink /tmp/l/alias2"), "/tmp/l/file\n");
    assert_eq!(run(&mut c, "ln /tmp/l /tmp/dirlink").exit_code, 1);
    assert_eq!(ok(&mut c, "realpath /tmp/l/soft"), "/tmp/l/hard\n");
}

#[test]
fn truncate_and_install_set_size_and_mode() {
    let mut c = machine();
    ok(&mut c, "truncate -s 5 /tmp/t");
    assert_eq!(ok(&mut c, "stat -c %s /tmp/t"), "5\n");
    ok(&mut c, "truncate -s +3 /tmp/t");
    assert_eq!(ok(&mut c, "stat -c %s /tmp/t"), "8\n");
    ok(&mut c, "truncate -s 2K /tmp/t");
    assert_eq!(ok(&mut c, "stat -c %s /tmp/t"), "2048\n");
    ok(&mut c, "truncate -s 0 /tmp/t");
    assert_eq!(ok(&mut c, "stat -c %s /tmp/t"), "0\n");
    refused(&mut c, "truncate /tmp/t", "-s");
    ok(&mut c, "echo prog > /tmp/prog");
    ok(&mut c, "install -m 755 -D /tmp/prog /tmp/bin/prog");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/bin/prog"), "755\n");
    assert_eq!(ok(&mut c, "cat /tmp/bin/prog"), "prog\n");
    ok(&mut c, "install -d -m 700 /tmp/private");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/private"), "700\n");
}

#[test]
fn every_documented_command_resolves() {
    let mut c = machine();
    // The roster `which` reports is the roster the matrix publishes.
    for name in [
        "ls",
        "cat",
        "stat",
        "find",
        "grep",
        "sed",
        "du",
        "df",
        "which",
        "nproc",
        "uptime",
        "clear",
        "ip",
        "sudo",
        "date",
        "wc",
        "sort",
        "uniq",
        "head",
        "tail",
        "cut",
        "tr",
        "awk",
        "xargs",
        "diff",
        "paste",
        "join",
        "comm",
        "tee",
        "nl",
        "rev",
        "fold",
        "expand",
        "unexpand",
        "shuf",
        "seq",
        "yes",
        "basename",
        "dirname",
        "realpath",
        "readlink",
        "split",
        "strings",
        "base64",
        "md5sum",
        "sha1sum",
        "sha256sum",
        "cmp",
        "xxd",
        "od",
        "hexdump",
        "file",
        "printf",
        "test",
        "ps",
        "break",
        "continue",
        "return",
        "kill",
        "git",
        "sh",
        "curl",
        "sleep",
        "systemctl",
        "apt",
        "sqlite3",
        "python3",
        "python",
        "node",
        "chown",
        "chgrp",
        "umask",
        "truncate",
        "install",
        "readlink",
        "realpath",
        "rmdir",
        "trash",
        "trash-list",
        "trash-restore",
        "trash-empty",
        "gio",
        "tar",
        "gzip",
        "gunzip",
        "zcat",
        "zip",
        "unzip",
        "rsync",
    ] {
        assert_eq!(
            ok(&mut c, &format!("which {name}")),
            format!("/usr/bin/{name}\n"),
            "{name} is in the matrix but not on PATH"
        );
    }
}

#[test]
fn control_flow_runs_every_construct() {
    let mut c = machine();
    assert_eq!(
        ok(&mut c, "if true; then echo yes; else echo no; fi"),
        "yes\n"
    );
    assert_eq!(
        ok(&mut c, "if false; then echo a; elif true; then echo b; fi"),
        "b\n"
    );
    assert_eq!(
        ok(&mut c, "if false; then echo a; else echo c; fi"),
        "c\n",
        "the else arm must run"
    );
    // An `if` whose condition fails and has no else is still a success.
    assert_eq!(run(&mut c, "if false; then echo a; fi").exit_code, 0);
    assert_eq!(ok(&mut c, "for i in a b c; do echo $i; done"), "a\nb\nc\n");
    assert_eq!(
        ok(
            &mut c,
            "n=0; while test $n -lt 3; do echo $n; n=$((n + 1)); done"
        ),
        "0\n1\n2\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "n=0; until test $n -ge 2; do echo $n; n=$((n + 1)); done"
        ),
        "0\n1\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "case hello in h*) echo star ;; *) echo rest ;; esac"
        ),
        "star\n"
    );
    assert_eq!(ok(&mut c, "case b in a|b) echo ab ;; esac"), "ab\n");
    assert_eq!(
        ok(&mut c, "case z in (a) echo a ;; *) echo other ;; esac"),
        "other\n"
    );
    // A case with no matching arm succeeds and prints nothing.
    let r = run(&mut c, "case z in a) echo a ;; esac");
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, ""));
    // A subshell restores the environment and the working directory.
    assert_eq!(
        ok(&mut c, "X=out; ( cd /tmp; X=in; pwd ); pwd; echo $X"),
        "/tmp\n/home/user\nout\n"
    );
    assert_eq!(ok(&mut c, "{ echo one; echo two; }"), "one\ntwo\n");
    // The `for` list is the one place a word is split and globbed.
    assert_eq!(
        ok(&mut c, "for f in /home/user/proj/*.txt; do echo $f; done"),
        "/home/user/proj/a.txt\n"
    );
    assert_eq!(
        ok(&mut c, "for w in $(echo p q); do echo $w; done"),
        "p\nq\n"
    );
    // A compound takes redirections and sits in a pipeline like any other command.
    assert_eq!(
        ok(
            &mut c,
            "for i in 1 2; do echo $i; done > /tmp/cf; cat /tmp/cf"
        ),
        "1\n2\n"
    );
    assert_eq!(
        ok(&mut c, "for i in 1 2 3; do echo $i; done | wc -l"),
        "3\n"
    );
    assert_eq!(ok(&mut c, "echo hi | { cat; echo more; }"), "hi\nmore\n");
    // A missing terminator is a syntax error, not a silently truncated script.
    refused(&mut c, "if true; then echo a", "fi");
    refused(&mut c, "for i in 1; do echo $i", "done");
    refused(&mut c, "case a in a) echo a", "esac");
    refused(&mut c, "echo )", ")");
}

#[test]
fn break_continue_and_return_are_signals() {
    let mut c = machine();
    assert_eq!(
        ok(
            &mut c,
            "for i in 1 2 3 4; do if test $i -eq 3; then break; fi; echo $i; done"
        ),
        "1\n2\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "for i in 1 2 3; do if test $i -eq 2; then continue; fi; echo $i; done"
        ),
        "1\n3\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "for a in 1 2; do for b in x y; do echo $a$b; break 2; done; done"
        ),
        "1x\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "for a in 1 2; do for b in x y; do echo $a$b; continue 2; done; done"
        ),
        "1x\n2x\n"
    );
    assert_eq!(ok(&mut c, "f() { return 3; }; f; echo $?"), "3\n");
    // Outside their construct they are refused rather than doing nothing.
    refused(&mut c, "break", "not in a loop");
    refused(&mut c, "continue", "not in a loop");
    refused(&mut c, "return", "not in a function");
    refused(&mut c, "for i in 1; do break x; done", "numeric");
}

#[test]
fn functions_carry_their_own_parameters() {
    let mut c = machine();
    assert_eq!(
        ok(&mut c, "greet() { echo hi $1; }; greet world"),
        "hi world\n"
    );
    assert_eq!(ok(&mut c, "function g { echo g; }; g"), "g\n");
    assert_eq!(
        ok(&mut c, "f() { echo \"$@ / $# / $1\"; }; f p q"),
        "p q / 2 / p\n"
    );
    // A function's output is the call's output, so it pipes and redirects.
    assert_eq!(ok(&mut c, "u() { echo quiet; }; u | tr a-z A-Z"), "QUIET\n");
    // Parameters are restored after the call.
    assert_eq!(
        ok(
            &mut c,
            "sh -c 'f() { echo $1; }; f inner; echo \"[$1]\"' zero outer"
        ),
        "inner\n[outer]\n"
    );
    // A function defined in a subshell does not escape it.
    assert_eq!(run(&mut c, "( s() { echo s; }; s ); s").exit_code, 127);
    // Unbounded recursion stops at the nesting limit rather than hanging.
    let r = run(&mut c, "r() { r; }; r");
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("nesting exceeds 32"), "{}", r.stderr);
    // `for NAME; do` walks the positional parameters.
    assert_eq!(
        ok(&mut c, "sh -c 'for a; do echo $a; done' zero one two"),
        "one\ntwo\n"
    );
}

#[test]
fn loops_are_bounded_rather_than_hanging() {
    let mut c = machine();
    let r = run(&mut c, "while true; do echo x; done");
    assert_eq!(r.exit_code, 2, "an endless loop must end with a status");
    assert!(r.stderr.contains("budget"), "{}", r.stderr);
    // `:` is the null command, so a loop body can legitimately do nothing.
    let r = run(&mut c, "for i in 1 2 3; do :; done");
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, ""));
    let r = run(&mut c, "until false; do :; done");
    assert_eq!(r.exit_code, 2, "an endless `until` must end too");
    // Nothing printed before the budget tripped survives as a partial success.
    let r = run(&mut c, "n=0; while true; do n=$((n + 1)); done; echo $n");
    assert_eq!(r.exit_code, 2);
    assert!(r.stdout.is_empty(), "{r:?}");
}

#[test]
fn sed_addresses_and_edit_commands() {
    let mut c = machine();
    let file = "/home/user/proj/a.txt";
    assert_eq!(ok(&mut c, &format!("sed -n '/beta/p' {file}")), "beta\n");
    assert_eq!(
        ok(&mut c, &format!("sed '/beta/d' {file}")),
        "alpha\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("sed -n '/beta/,/gamma/p' {file}")),
        "beta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("sed '/beta/s/e/E/' {file}")),
        "alpha\nbEta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("sed '2a added' {file}")),
        "alpha\nbeta\nadded\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("sed '$a tail' {file}")),
        "alpha\nbeta\ngamma\ntail\n"
    );
    assert_eq!(
        ok(&mut c, &format!("sed '1i head' {file}")),
        "head\nalpha\nbeta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("sed 'y/abc/ABC/' {file}")),
        "AlphA\nBetA\ngAmmA\n"
    );
    assert_eq!(ok(&mut c, &format!("sed '2q' {file}")), "alpha\nbeta\n");
    // `q CODE` becomes the exit status, and what it printed still arrives.
    let r = run(&mut c, &format!("sed '2q5' {file}"));
    assert_eq!((r.exit_code, r.stdout.as_str()), (5, "alpha\nbeta\n"));
    refused(&mut c, &format!("sed 'y/ab/x/' {file}"), "equal length");
    refused(&mut c, &format!("sed '/unclosed' {file}"), "unterminated");
    refused(&mut c, &format!("sed '0p' {file}"), "line address 0");
}

#[test]
fn grep_shows_context_and_only_the_match() {
    let mut c = machine();
    let file = "/home/user/proj/a.txt";
    assert_eq!(
        ok(&mut c, &format!("grep -A1 beta {file}")),
        "beta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("grep -B1 gamma {file}")),
        "beta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, &format!("grep -C1 beta {file}")),
        "alpha\nbeta\ngamma\n"
    );
    // A context line is marked `-` where a matching line is marked `:`.
    assert_eq!(
        ok(&mut c, &format!("grep -n -A1 beta {file}")),
        "2:beta\n3-gamma\n"
    );
    // Non-adjacent groups are separated by `--`.
    let split = ok(&mut c, &format!("grep -A0 -e alpha -e gamma {file}"));
    assert_eq!(split, "alpha\n--\ngamma\n", "{split}");
    assert_eq!(ok(&mut c, &format!("grep -o 'a.' {file}")), "al\nam\n");
    assert_eq!(
        ok(&mut c, &format!("grep -o -n 'a.' {file}")),
        "1:al\n3:am\n"
    );
    refused(&mut c, &format!("grep -A x beta {file}"), "-A");
    refused(&mut c, &format!("grep -C q beta {file}"), "-C");
}

#[test]
fn chmod_accepts_symbolic_and_recursive_modes() {
    let mut c = machine();
    let mode = |c: &mut Computer| ok(c, "stat -c %a /tmp/m");
    ok(&mut c, "echo x > /tmp/m; chmod 600 /tmp/m");
    assert_eq!(mode(&mut c), "600\n");
    ok(&mut c, "chmod u+x /tmp/m");
    assert_eq!(mode(&mut c), "700\n");
    ok(&mut c, "chmod go+r /tmp/m");
    assert_eq!(mode(&mut c), "744\n");
    ok(&mut c, "chmod go-r /tmp/m");
    assert_eq!(mode(&mut c), "700\n");
    ok(&mut c, "chmod a=r /tmp/m");
    assert_eq!(mode(&mut c), "444\n", "`=` clears what it does not set");
    ok(&mut c, "chmod u+w,g+x /tmp/m");
    assert_eq!(mode(&mut c), "654\n");
    ok(&mut c, "chmod 644 /tmp/m; chmod +x /tmp/m");
    assert_eq!(mode(&mut c), "755\n", "a bare `+x` means `a+x`");
    ok(&mut c, "chmod u+s /tmp/m");
    assert_eq!(mode(&mut c), "4755\n");
    // -R walks the tree; X only grants execute where one already exists.
    ok(
        &mut c,
        "mkdir -p /tmp/tree/sub; echo y > /tmp/tree/sub/plain; chmod -R 600 /tmp/tree",
    );
    ok(&mut c, "chmod -R a+X /tmp/tree");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/tree"), "711\n");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/tree/sub/plain"), "600\n");
    refused(&mut c, "chmod u+z /tmp/m", "invalid mode");
    refused(&mut c, "chmod u=g /tmp/m", "copying permissions");
    refused(&mut c, "chmod 999 /tmp/m", "invalid octal");
    refused(&mut c, "chmod 644", "missing operand");
    refused(&mut c, "chmod --reference=/tmp/m /tmp/m", "--reference");
    assert_eq!(run(&mut c, "chmod 600 /nope").exit_code, 1);
}

#[test]
fn touch_sets_the_timestamp_it_is_given() {
    let mut c = machine();
    let stamp = |c: &mut Computer| ok(c, "stat -c %y /tmp/t");
    // With no option the file takes the current simulated tick, not the host clock.
    let later = shell::execute(&mut c, "touch /tmp/t", 86_400_000_000, &mut OfflineHost);
    assert_eq!(later.exit_code, 0, "{}", later.stderr);
    assert_eq!(stamp(&mut c), "2026-09-18 09:00:00.000000000 +0000\n");
    // An existing file keeps its bytes and moves its timestamp.
    ok(
        &mut c,
        "echo body > /tmp/t; touch -d '2026-09-19 01:02:03' /tmp/t",
    );
    assert_eq!(ok(&mut c, "cat /tmp/t"), "body\n");
    assert_eq!(stamp(&mut c), "2026-09-19 01:02:03.000000000 +0000\n");
    ok(&mut c, "touch -t 202609201122.33 /tmp/t");
    assert_eq!(stamp(&mut c), "2026-09-20 11:22:33.000000000 +0000\n");
    ok(&mut c, "touch -r /home/user/proj/a.txt /tmp/t");
    assert_eq!(stamp(&mut c), "2026-09-17 09:00:00.000000000 +0000\n");
    // -c never creates.
    ok(&mut c, "touch -c /tmp/absent");
    assert_eq!(run(&mut c, "test -e /tmp/absent").exit_code, 1);
    refused(&mut c, "touch -d yesterday /tmp/t", "-d");
    refused(&mut c, "touch -d 1999-01-01 /tmp/t", "epoch");
    refused(&mut c, "touch -t 99 /tmp/t", "-t");
    refused(&mut c, "touch", "missing operand");
}

#[test]
fn ps_prints_columns_by_default() {
    let mut c = machine();
    let table = ok(&mut c, "ps");
    assert_eq!(
        table.lines().next(),
        Some("    PID TTY          TIME CMD"),
        "a person typing `ps` expects columns: {table}"
    );
    assert!(!table.starts_with('['), "the default must not be JSON");
    assert!(
        table.lines().nth(1).is_some_and(|l| l.ends_with(" ps")),
        "{table}"
    );
    // -e reaches init, which the default (this user's processes) does not.
    assert!(!table.contains("init"), "{table}");
    let all = ok(&mut c, "ps -e");
    assert!(all.contains(" init"), "{all}");
    let full = ok(&mut c, "ps -ef");
    assert_eq!(
        full.lines().next(),
        Some("UID          PID    PPID  C STIME TTY          TIME CMD")
    );
    assert!(full.lines().any(|l| l.starts_with("root")), "{full}");
    assert_eq!(ok(&mut c, "ps -p 1").lines().count(), 2);
    assert!(ok(&mut c, "ps -u root").contains("init"));
    // The JSON dump is still reachable, behind a flag.
    let json = ok(&mut c, "ps --json");
    assert!(json.starts_with('['), "{json}");
    assert!(json.contains("\"pid\""), "{json}");
    // BSD syntax, the column selector and the sort key.
    let aux = ok(&mut c, "ps aux");
    assert_eq!(
        aux.lines().next(),
        Some("USER         PID %CPU %MEM     VSZ    RSS TTY      STAT START     TIME COMMAND")
    );
    assert!(aux.lines().any(|l| l.starts_with("root")), "{aux}");
    let chosen = ok(&mut c, "ps -e -o pid,rss,comm");
    assert_eq!(chosen.lines().next(), Some("    PID    RSS COMMAND"));
    assert!(chosen.lines().any(|l| l.ends_with(" init")), "{chosen}");
    // The report's question: the biggest processes, largest first.
    let biggest = ok(&mut c, "ps -e -o rss,comm --sort=-rss");
    let sizes: Vec<u64> = biggest
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect();
    assert!(
        sizes.windows(2).all(|w| w[0] >= w[1]),
        "--sort=-rss must order by size: {biggest}"
    );
    refused(&mut c, "ps -o nonsense", "nonsense");
    refused(&mut c, "ps -e --sort=nonsense", "nonsense");
    refused(&mut c, "ps -p x", "-p");
}

#[test]
fn the_process_table_answers_what_is_running_and_what_it_costs() {
    let mut c = machine();
    // Every column `-o` names is read from the table, and RSS is the published model.
    let row = ok(
        &mut c,
        "ps -p 1 -o pid,ppid,user,stat,tty,rss,vsz,pmem,etimes,comm",
    );
    let cells: Vec<&str> = row.lines().nth(1).unwrap().split_whitespace().collect();
    assert_eq!(cells[0], "1", "{row}");
    assert_eq!(cells[2], "root", "{row}");
    assert_eq!(cells[3], "R", "{row}");
    assert_eq!(cells[4], "?", "init is on no terminal: {row}");
    assert_eq!(cells[5], "2048", "init holds the published 2 MiB: {row}");
    assert_eq!(
        cells[6], "67584",
        "VSZ is RSS plus the fixed mapping: {row}"
    );
    assert_eq!(cells.last(), Some(&"init"), "{row}");
    // A command typed at the shell runs on the machine's one pseudo-terminal.
    assert!(ok(&mut c, "ps").contains("pts/0"));
    // free totals the same footprints ps reports.
    let free = ok(&mut c, "free -b");
    let mem: Vec<&str> = free
        .lines()
        .find(|l| l.starts_with("Mem:"))
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(mem[1], (8u64 << 30).to_string(), "{free}");
    assert!(mem[2].parse::<u64>().unwrap() > 0, "{free}");
    // top is a batch snapshot of the same table, and refuses to pretend otherwise.
    let top = ok(&mut c, "top -b -n1");
    assert!(top.starts_with("top - "), "{top}");
    assert!(top.contains("MiB Mem :"), "{top}");
    assert!(top.lines().any(|l| l.ends_with(" init")), "{top}");
    refused(&mut c, "top", "interactive");
    refused(&mut c, "top -b -n5", "-n 5");
    // What cannot be modelled is refused by name, never faked.
    refused(&mut c, "nice -n 5 ls", "scheduler");
    refused(&mut c, "jobs", "job control");
    refused(&mut c, "vmstat", "counters");
    // And "how much disk is left" is answerable from the same session.
    assert!(ok(&mut c, "df -h").contains("/"));
}

#[test]
fn the_machine_lists_its_applications_and_hands_a_document_to_the_desktop() {
    let mut c = machine();
    // A machine with nothing installed cannot open anything, and says so.
    refused_with(
        &mut c,
        "xdg-open /home/user/proj/a.txt",
        3,
        "no application",
    );
    c.installed_apps = ["terminal", "editor", "files"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(ok(&mut c, "apps"), "editor\nfiles\nterminal\n");
    assert!(ok(&mut c, "apps --json").starts_with("[\"editor\""));
    // The shell records the request; opening a window is the desktop's job.
    let opened = run(&mut c, "xdg-open /home/user/proj/a.txt");
    assert_eq!(opened.exit_code, 0, "{}", opened.stderr);
    assert_eq!(opened.open, ["/home/user/proj/a.txt"], "{opened:?}");
    assert_eq!(
        run(&mut c, "xdg-open https://example.com/").open,
        ["https://example.com/"]
    );
    // A nested shell's request is carried out with everything else it produced.
    assert_eq!(
        run(&mut c, "sh -c 'xdg-open /home/user/proj/a.txt'").open,
        ["/home/user/proj/a.txt"]
    );
    // What a shell can check, it checks.
    refused_with(&mut c, "xdg-open /home/user/nowhere", 2, "no such file");
    refused(&mut c, "xdg-open -n /home/user/proj/a.txt", "-n");
    refused(&mut c, "xdg-open a b", "exactly one");
}

#[test]
fn signals_reach_what_is_running() {
    let mut c = machine();
    assert!(ok(&mut c, "kill -l").contains("TERM"));
    // A background sleep is a real process; pgrep finds it and pkill ends it.
    ok(&mut c, "sleep 60 &");
    let found = ok(&mut c, "pgrep -f sleep");
    let pid: u64 = found.trim().parse().expect(&found);
    assert!(ok(&mut c, "ps -e").contains(&format!("{pid} ")), "{found}");
    ok(&mut c, "pkill -KILL -f sleep");
    assert!(
        !ok(&mut c, "ps -e").contains("sleep 60"),
        "a killed process must leave the table"
    );
    // pgrep says nothing and exits 1 when nothing matches, as it does on Linux.
    let empty = run(&mut c, "pgrep -f sleep");
    assert_eq!(empty.exit_code, 1, "{empty:?}");
    assert!(empty.stdout.is_empty(), "{empty:?}");
    // A pattern this world cannot honour is refused rather than half-matched.
    refused(&mut c, "pgrep 'sle.*p'", "regular expressions");
}

#[test]
fn read_walks_a_shared_input_stream() {
    let mut c = machine();
    ok(&mut c, "printf 'a b c\\nd e f\\n' > /tmp/rows");
    assert_eq!(
        ok(&mut c, "while read l; do echo [$l]; done < /tmp/rows"),
        "[a b c]\n[d e f]\n"
    );
    // Several names split the line; the last one takes the remainder.
    assert_eq!(
        ok(
            &mut c,
            "while read x y; do echo \"$x|$y\"; done < /tmp/rows"
        ),
        "a|b c\nd|e f\n"
    );
    // A pipe into a compound is the same stream.
    assert_eq!(
        ok(&mut c, "cat /tmp/rows | while read a b c; do echo $c; done"),
        "c\nf\n"
    );
    // No name sets REPLY.
    assert_eq!(ok(&mut c, "read < /tmp/rows; echo $REPLY"), "a b c\n");
    // `read` advances the stream, other commands consume the rest of it.
    assert_eq!(ok(&mut c, "{ read a; cat; } < /tmp/rows"), "d e f\n");
    // A loop body that ignores stdin does not swallow the loop's own lines.
    assert_eq!(
        ok(
            &mut c,
            "n=0; while read l; do n=$((n + 1)); done < /tmp/rows; echo $n"
        ),
        "2\n"
    );
    // End of input is status 1, which is what stops the loop.
    assert_eq!(run(&mut c, "read x < /dev/null").exit_code, 1);
    assert_eq!(run(&mut c, "echo one | read x; echo $x").stdout, "one\n");
    refused(&mut c, "read -p prompt x", "invalid option -- 'p'");
    refused(&mut c, "read -t 5 x", "invalid option -- 't'");
    refused(&mut c, "read 1bad < /tmp/rows", "not a valid name");
}

#[test]
fn exit_ends_the_shell_it_is_in_and_nothing_more() {
    let mut c = machine();
    let r = run(&mut c, "echo a; exit 7; echo b");
    assert_eq!((r.exit_code, r.stdout.as_str()), (7, "a\n"));
    // Bare exit carries the last status.
    assert_eq!(run(&mut c, "false; exit").exit_code, 1);
    assert_eq!(run(&mut c, "true; exit").exit_code, 0);
    // It escapes a function and a loop, unlike `return` and `break`.
    let r = run(&mut c, "f() { exit 5; }; f; echo never");
    assert_eq!((r.exit_code, r.stdout.as_str()), (5, ""));
    let r = run(&mut c, "for i in 1 2 3; do echo $i; exit 3; done");
    assert_eq!((r.exit_code, r.stdout.as_str()), (3, "1\n"));
    // It does not escape a subshell or a nested shell.
    assert_eq!(ok(&mut c, "( exit 6 ); echo $?"), "6\n");
    assert_eq!(ok(&mut c, "sh -c 'exit 9'; echo $?"), "9\n");
    assert_eq!(
        ok(&mut c, "echo 'exit 4' > /tmp/x.sh; sh /tmp/x.sh; echo $?"),
        "4\n"
    );
    refused(&mut c, "exit later", "numeric");
}

#[test]
fn shift_local_and_source_complete_the_scope_story() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "sh -c 'shift; echo \"$1 $#\"' s a b c"), "b 2\n");
    assert_eq!(
        ok(&mut c, "sh -c 'shift 2; echo \"$1 $#\"' s a b c"),
        "c 1\n"
    );
    // Shifting past the end is a modelled negative and changes nothing.
    assert_eq!(ok(&mut c, "sh -c 'shift 9; echo \"$? $#\"' s a b"), "1 2\n");
    refused(&mut c, "shift x", "numeric");
    // local shadows only for the call, including a name that did not exist.
    assert_eq!(
        ok(
            &mut c,
            "f() { local V=in W; echo \"[$V][$W]\"; }; V=out; W=keep; f; echo \"[$V][$W]\""
        ),
        "[in][]\n[out][keep]\n"
    );
    refused(&mut c, "local X=1", "not in a function");
    refused(&mut c, "f() { local 1bad; }; f", "not a valid name");
    // source runs in this shell: variables, cwd and functions all persist.
    ok(
        &mut c,
        "printf 'SV=5\\ngreet() { echo hi; }\\n' > /tmp/lib.sh",
    );
    assert_eq!(ok(&mut c, "source /tmp/lib.sh; echo $SV; greet"), "5\nhi\n");
    assert_eq!(ok(&mut c, ". /tmp/lib.sh; echo $SV"), "5\n");
    // return ends a sourced file; exit ends the shell that sourced it.
    ok(&mut c, "echo 'return 3' > /tmp/r.sh");
    assert_eq!(ok(&mut c, "source /tmp/r.sh; echo $?"), "3\n");
    ok(&mut c, "echo 'exit 2' > /tmp/e.sh");
    let r = run(&mut c, "source /tmp/e.sh; echo never");
    assert_eq!((r.exit_code, r.stdout.as_str()), (2, ""));
    assert_eq!(run(&mut c, "source /nope").exit_code, 1);
    refused(&mut c, "source", "missing file");
}

#[test]
fn double_brackets_extend_test_rather_than_replacing_it() {
    let mut c = machine();
    let file = "/home/user/proj/a.txt";
    assert_eq!(ok(&mut c, &format!("[[ -f {file} ]] && echo yes")), "yes\n");
    // An unquoted right side of == is a pattern, a quoted one is a literal.
    assert_eq!(ok(&mut c, "[[ abc == a* ]] && echo glob"), "glob\n");
    assert_eq!(ok(&mut c, "[[ abc == 'a*' ]] || echo literal"), "literal\n");
    assert_eq!(ok(&mut c, "[[ abc != x* ]] && echo differs"), "differs\n");
    assert_eq!(ok(&mut c, "[[ abc =~ ^a.c$ ]] && echo regex"), "regex\n");
    assert_eq!(ok(&mut c, "[[ a < b ]] && echo order"), "order\n");
    // The operators inside the brackets are the test's, not the shell's.
    assert_eq!(
        ok(&mut c, "[[ 3 -gt 2 && 1 -lt 2 ]] && echo both"),
        "both\n"
    );
    assert_eq!(
        ok(&mut c, "[[ 1 -eq 2 || 2 -eq 2 ]] && echo either"),
        "either\n"
    );
    assert_eq!(ok(&mut c, "[[ ! -f /nope ]] && echo negated"), "negated\n");
    assert_eq!(
        ok(
            &mut c,
            "[[ ( 1 -eq 1 || 2 -eq 3 ) && 4 -eq 4 ]] && echo grouped"
        ),
        "grouped\n"
    );
    assert_eq!(run(&mut c, "[[ -f /nope ]]").exit_code, 1);
    // test and [ gained the same primaries.
    assert_eq!(
        ok(&mut c, &format!("test -s {file} && echo nonempty")),
        "nonempty\n"
    );
    assert_eq!(
        ok(&mut c, &format!("test -r {file} && echo readable")),
        "readable\n"
    );
    assert_eq!(
        ok(&mut c, &format!("[ -x {file} ] || echo notexec")),
        "notexec\n"
    );
    assert_eq!(
        ok(&mut c, "test -L /home/user/link && echo symlink"),
        "symlink\n"
    );
    ok(&mut c, "touch -d 2026-09-20 /tmp/newer");
    assert_eq!(
        ok(&mut c, &format!("test /tmp/newer -nt {file} && echo newer")),
        "newer\n"
    );
    refused(&mut c, "[[ -f /tmp", "]]");
    refused(&mut c, "[[ 1 -zz 2 ]]", "[[");
    refused(&mut c, &format!("test -G {file}"), "-G");
}

#[test]
fn getopts_parses_an_option_string() {
    let mut c = machine();
    assert_eq!(
        ok(&mut c, "sh -c 'while getopts ab:c o; do echo \"$o=$OPTARG\"; done; echo $OPTIND' s -a -b val -c"),
        "a=\nb=val\nc=\n5\n"
    );
    // Clusters and glued arguments both work.
    assert_eq!(
        ok(
            &mut c,
            "sh -c 'while getopts ab: o; do echo \"$o=$OPTARG\"; done' s -ab val"
        ),
        "a=\nb=val\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "sh -c 'while getopts b: o; do echo $OPTARG; done' s -bglued"
        ),
        "glued\n"
    );
    // A leading colon reports unknown options quietly through OPTARG.
    assert_eq!(
        ok(
            &mut c,
            "sh -c 'while getopts :ab: o; do echo \"$o/$OPTARG\"; done' s -z"
        ),
        "?/z\n"
    );
    let noisy = run(&mut c, "sh -c 'while getopts ab: o; do :; done' s -z");
    assert!(noisy.stderr.contains("illegal option -- z"), "{noisy:?}");
    let missing = run(&mut c, "sh -c 'while getopts b: o; do :; done' s -b");
    assert!(
        missing.stderr.contains("requires an argument"),
        "{missing:?}"
    );
    // `--` ends the options and leaves OPTIND past it.
    assert_eq!(
        ok(
            &mut c,
            "sh -c 'while getopts a o; do echo $o; done; echo $OPTIND' s -a -- x"
        ),
        "a\n3\n"
    );
    refused(&mut c, "getopts", "usage");
    refused(&mut c, "getopts ab 1bad", "not a valid name");
}

#[test]
fn unquoted_expansions_split_into_fields() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "X='a b'; sh -c 'echo $#' s $X"), "2\n");
    assert_eq!(ok(&mut c, "X='a b'; sh -c 'echo $#' s \"$X\""), "1\n");
    // An empty unquoted expansion contributes no argument; a quoted one contributes ''.
    assert_eq!(ok(&mut c, "E=; sh -c 'echo $#' s $E"), "0\n");
    assert_eq!(ok(&mut c, "E=; sh -c 'echo $#' s \"$E\""), "1\n");
    assert_eq!(
        ok(&mut c, "X='a b'; for w in $X; do echo $w; done"),
        "a\nb\n"
    );
    // Adjacent literal text joins the first and last fields, as in bash.
    assert_eq!(
        ok(&mut c, "X='a b'; sh -c 'echo \"$1|$2\"' s p${X}q"),
        "pa|bq\n"
    );
    // A `*` arriving from a variable stays literal; one in the source globs.
    assert_eq!(ok(&mut c, "P='*.txt'; echo $P"), "*.txt\n");
    assert_eq!(ok(&mut c, "cd /home/user/proj; echo *.txt"), "a.txt\n");
}

#[test]
fn head_and_tail_take_a_bare_count_as_well_as_dash_n() {
    // `head -3` is what people type; it is the historical spelling of `head -n 3`.
    let mut c = machine();
    run(&mut c, "printf 'a\\nb\\nc\\nd\\ne\\n' > /tmp/five.txt");
    assert_eq!(ok(&mut c, "head -2 /tmp/five.txt"), "a\nb\n");
    assert_eq!(ok(&mut c, "head -n 2 /tmp/five.txt"), "a\nb\n");
    assert_eq!(ok(&mut c, "tail -2 /tmp/five.txt"), "d\ne\n");
    assert_eq!(ok(&mut c, "cat /tmp/five.txt | head -1"), "a\n");
    // A count that is not a count still fails loudly rather than being ignored.
    refused(&mut c, "head -x /tmp/five.txt", "x");
}

#[test]
fn sqlite3_keeps_real_database_files_on_the_machine() {
    let mut c = machine();
    ok(
        &mut c,
        "sqlite3 /home/user/shop.db 'create table t(a, b); insert into t values (1, 2), (3, 4)'",
    );
    assert_eq!(
        ok(&mut c, "sqlite3 /home/user/shop.db 'select a + b from t'"),
        "3\n7\n"
    );
    // A real SQLite 3 file: whole 4 KiB pages, table and schema on separate ones.
    assert_eq!(ok(&mut c, "stat -c %s /home/user/shop.db"), "8192\n");
    assert_eq!(
        ok(&mut c, "grep -c 'SQLite format 3' /home/user/shop.db"),
        "1\n"
    );
    // SQL and dot-commands arrive on standard input from a pipe, a heredoc or a file.
    assert_eq!(
        ok(
            &mut c,
            "printf '.tables\\nselect count(*) from t;\\n' | sqlite3 /home/user/shop.db"
        ),
        "t\n2\n"
    );
    assert_eq!(
        ok(&mut c, "cd /home/user; sqlite3 shop.db <<EOF\n.headers on\n.mode csv\nselect * from t where a = 3;\nEOF"),
        "a,b\r\n3,4\r\n"
    );
    ok(&mut c, "printf 'x,y\\n5,6\\n' > /home/user/in.csv");
    assert_eq!(
        ok(&mut c, "sqlite3 /home/user/shop.db '.import --csv /home/user/in.csv pts' 'select x * y from pts'"),
        "30\n"
    );
    // Reading never creates a database; writing does.
    assert_eq!(ok(&mut c, "sqlite3 /home/user/none.db 'select 1'"), "1\n");
    assert_eq!(run(&mut c, "test -e /home/user/none.db").exit_code, 1);
    // 'now' is the simulated clock, never the host's.
    assert_eq!(
        ok(&mut c, "sqlite3 :memory: \"select datetime('now')\""),
        "2026-09-17 09:00:00\n"
    );
    let r = run(&mut c, "sqlite3 /home/user/shop.db 'select * from missing'");
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stderr, "Error: in prepare, no such table: missing\n");
    refused(&mut c, "sqlite3 -bogus /home/user/shop.db", "-bogus");
    // The file's permissions apply: a read-only database refuses writes.
    ok(&mut c, "chmod 444 /home/user/shop.db");
    let r = run(&mut c, "sqlite3 /home/user/shop.db 'delete from t'");
    assert_ne!(r.exit_code, 0);
    assert_eq!(
        ok(
            &mut c,
            "sqlite3 /home/user/shop.db 'select count(*) from t'"
        ),
        "2\n"
    );
}

#[test]
fn python3_runs_programs_against_the_machine() {
    let mut c = machine();
    // Inline code, version, and the conventional aliases.
    assert_eq!(ok(&mut c, "python3 -c 'print(6 * 7)'"), "42\n");
    assert_eq!(ok(&mut c, "python --version"), "Python 3.12.3\n");
    assert_eq!(
        ok(
            &mut c,
            "/usr/bin/python3 -c 'import sys; print(sys.argv)' x"
        ),
        "['-c', 'x']\n"
    );
    // A script file with arguments sees the simulated filesystem and cwd.
    run(
        &mut c,
        "printf 'import sys, os\\nprint(sys.argv[1:], os.getcwd())\\nprint(open(\"proj/a.txt\").read().split())\\n' > /home/user/main.py",
    );
    assert_eq!(
        ok(&mut c, "cd /home/user && python3 main.py one two"),
        "['one', 'two'] /home/user\n['alpha', 'beta', 'gamma']\n"
    );
    // Pipes feed stdin; output flows on through the pipeline.
    assert_eq!(
        ok(
            &mut c,
            "cat /home/user/proj/a.txt | python3 -c 'import sys; print(len(sys.stdin.read().splitlines()))'"
        ),
        "3\n"
    );
    assert_eq!(
        ok(&mut c, "python3 -c 'for i in range(3): print(i)' | wc -l"),
        "3\n"
    );
    assert_eq!(ok(&mut c, "echo 'print(1 + 1)' | python3"), "2\n");
    // Files a program writes land in the VFS.
    ok(
        &mut c,
        "python3 -c 'open(\"/tmp/py-out.txt\", \"w\").write(\"from python\\n\")'",
    );
    assert_eq!(ok(&mut c, "cat /tmp/py-out.txt"), "from python\n");
    // Redirection of the runtime's own streams.
    run(
        &mut c,
        "python3 -c 'import sys; print(\"err\", file=sys.stderr); print(\"out\")' > /tmp/o 2> /tmp/e",
    );
    assert_eq!(ok(&mut c, "cat /tmp/o"), "out\n");
    assert_eq!(ok(&mut c, "cat /tmp/e"), "err\n");
    // A program's chdir does not move the shell.
    ok(
        &mut c,
        "cd /home/user && python3 -c 'import os; os.chdir(\"/tmp\")'",
    );
    assert_eq!(
        ok(&mut c, "cd /home/user && python3 -c 'pass' && pwd"),
        "/home/user\n"
    );
}

#[test]
fn python3_reports_errors_and_exit_codes_like_cpython() {
    let mut c = machine();
    run(
        &mut c,
        "printf 'def f(x):\\n    return x / 0\\n\\nprint(\"before\")\\nf(1)\\n' > /home/user/bad.py",
    );
    let r = run(&mut c, "cd /home/user && python3 bad.py");
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stdout, "before\n");
    assert_eq!(
        r.stderr,
        "Traceback (most recent call last):\n  File \"/home/user/bad.py\", line 5, in <module>\n    f(1)\n  File \"/home/user/bad.py\", line 2, in f\n    return x / 0\n           ~~^~~\nZeroDivisionError: division by zero\n"
    );
    assert_eq!(
        run(&mut c, "python3 -c 'import sys; sys.exit(7)'").exit_code,
        7
    );
    assert_eq!(
        run(&mut c, "python3 -c 'raise SystemExit(\"bye\")'").stderr,
        "bye\n"
    );
    let r = run(&mut c, "python3 /nope.py");
    assert_eq!(r.exit_code, 2);
    assert_eq!(
        r.stderr,
        "/usr/bin/python3: can't open file '/nope.py': [Errno 2] No such file or directory\n"
    );
    // The shell's status logic sees the runtime's status.
    assert_eq!(
        ok(
            &mut c,
            "python3 -c 'import sys; sys.exit(1)' || echo failed"
        ),
        "failed\n"
    );
    // An infinite loop ends deterministically on the step budget.
    let r = run(&mut c, "python3 -c 'while True: pass'");
    assert_eq!(r.exit_code, 124);
    assert!(r
        .stderr
        .contains("TimeoutError: execution step limit exceeded"));
}

#[test]
fn node_runs_programs_against_the_machine() {
    let mut c = machine();
    assert_eq!(ok(&mut c, "node -e 'console.log(6 * 7)'"), "42\n");
    assert_eq!(ok(&mut c, "node --version"), "v24.21.0\n");
    assert_eq!(ok(&mut c, "node -p '[1, 2].map(x => x * 2)'"), "[ 2, 4 ]\n");
    assert_eq!(
        ok(
            &mut c,
            "/usr/bin/node -e 'console.log(process.argv.slice(1))' a b"
        ),
        "[ 'a', 'b' ]\n"
    );
    // A script file with arguments sees the simulated filesystem and cwd.
    run(
        &mut c,
        "printf 'const fs = require(\"fs\");\\nconsole.log(process.argv.slice(2), process.cwd());\\nconsole.log(fs.readFileSync(\"proj/a.txt\", \"utf8\").trim().split(String.fromCharCode(10)));\\n' > /home/user/app.js",
    );
    assert_eq!(
        ok(&mut c, "cd /home/user && node app.js one two"),
        "[ 'one', 'two' ] /home/user\n[ 'alpha', 'beta', 'gamma' ]\n"
    );
    // Pipes feed stdin (fs.readFileSync(0) and process.stdin); the program
    // itself can arrive on stdin.
    assert_eq!(
        ok(
            &mut c,
            "cat /home/user/proj/a.txt | node -e 'console.log(require(\"fs\").readFileSync(0, \"utf8\").split(\"\\n\").length)'"
        ),
        "4\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "printf '3\\n4\\n' | node -e 'let d = \"\"; process.stdin.on(\"data\", c => d += c).on(\"end\", () => console.log(d.split(\"\\n\").filter(Boolean).map(Number).reduce((a, b) => a + b)))'"
        ),
        "7\n"
    );
    assert_eq!(ok(&mut c, "echo 'console.log(1 + 1)' | node"), "2\n");
    assert_eq!(
        ok(
            &mut c,
            "node -e 'for (let i = 0; i < 3; i++) console.log(i)' | wc -l"
        ),
        "3\n"
    );
    // Files a program writes land in the VFS.
    ok(
        &mut c,
        "node -e 'require(\"fs\").writeFileSync(\"/tmp/js-out.txt\", \"from node\\n\")'",
    );
    assert_eq!(ok(&mut c, "cat /tmp/js-out.txt"), "from node\n");
    // Redirection of the runtime's own streams.
    run(
        &mut c,
        "node -e 'console.error(\"err\"); console.log(\"out\")' > /tmp/o 2> /tmp/e",
    );
    assert_eq!(ok(&mut c, "cat /tmp/o"), "out\n");
    assert_eq!(ok(&mut c, "cat /tmp/e"), "err\n");
    // Timers and promises run to completion before the command ends.
    assert_eq!(
        ok(&mut c, "node -e 'setTimeout(() => console.log(\"later\"), 50); Promise.resolve().then(() => console.log(\"soon\"))'"),
        "soon\nlater\n"
    );
    // A program's chdir does not move the shell.
    ok(&mut c, "cd /home/user && node -e 'process.chdir(\"/tmp\")'");
    assert_eq!(
        ok(&mut c, "cd /home/user && node -e '0' && pwd"),
        "/home/user\n"
    );
}

#[test]
fn node_reports_errors_and_exit_codes_like_node() {
    let mut c = machine();
    run(
        &mut c,
        "printf 'function f(x) {\\n  return x.y.z;\\n}\\nconsole.log(\"before\");\\nf({});\\n' > /home/user/bad.js",
    );
    let r = run(&mut c, "cd /home/user && node bad.js");
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stdout, "before\n");
    assert_eq!(
        r.stderr,
        "/home/user/bad.js:2\n  return x.y.z;\n             ^\n\nTypeError: Cannot read properties of undefined (reading 'z')\n    at f (/home/user/bad.js:2:14)\n    at Object.<anonymous> (/home/user/bad.js:5:1)\n    at Module._compile (node:internal/modules/cjs/loader:1929:14)\n    at Object..js (node:internal/modules/cjs/loader:2060:10)\n    at Module.load (node:internal/modules/cjs/loader:1651:32)\n    at Module._load (node:internal/modules/cjs/loader:1443:12)\n    at wrapModuleLoad (node:internal/modules/cjs/loader:261:19)\n    at Module.executeUserEntryPoint [as runMain] (node:internal/modules/run_main:154:5)\n    at node:internal/main/run_main_module:33:47\n\nNode.js v24.21.0\n"
    );
    assert_eq!(run(&mut c, "node -e 'process.exit(7)'").exit_code, 7);
    assert_eq!(run(&mut c, "node -e 'process.exitCode = 3'").exit_code, 3);
    let r = run(&mut c, "cd /home/user && node nope.js");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.starts_with("node:internal/modules/cjs/loader:1568\n  throw err;\n  ^\n\nError: Cannot find module '/home/user/nope.js'\n"));
    let r = run(&mut c, "node -e 'let x = ;'");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("SyntaxError: Unexpected token ';'"));
    // The shell's status logic sees the runtime's status.
    assert_eq!(
        ok(&mut c, "node -e 'process.exit(1)' || echo failed"),
        "failed\n"
    );
    // An infinite loop ends deterministically on the step budget.
    let r = run(&mut c, "node -e 'while (true) {}'");
    assert_eq!(r.exit_code, 124);
    assert!(r
        .stderr
        .contains("TimeoutError: execution step limit exceeded"));
}

#[test]
fn shebang_scripts_run_under_their_interpreter() {
    let mut c = machine();
    run(
        &mut c,
        "printf '#!/usr/bin/env python3\\nimport sys\\nprint(\"hi from\", sys.argv[0], sys.argv[1:])\\n' > /home/user/hello; chmod 755 /home/user/hello",
    );
    assert_eq!(
        ok(&mut c, "/home/user/hello a b"),
        "hi from /home/user/hello ['a', 'b']\n"
    );
    run(
        &mut c,
        "mkdir -p /home/user/bin; printf '#!/usr/bin/python3\\nprint(\"on PATH\")\\n' > /home/user/bin/tool; chmod +x /home/user/bin/tool",
    );
    assert_eq!(ok(&mut c, "PATH=/home/user/bin:/usr/bin tool"), "on PATH\n");
    run(
        &mut c,
        "printf '#!/usr/bin/env node\\nconsole.log(\"js\", process.argv.slice(2))\\n' > /home/user/bin/jstool; chmod +x /home/user/bin/jstool",
    );
    assert_eq!(
        ok(&mut c, "PATH=/home/user/bin:/usr/bin jstool x"),
        "js [ 'x' ]\n"
    );
    assert_eq!(
        ok(&mut c, "which python3 node"),
        "/usr/bin/python3\n/usr/bin/node\n"
    );
}

/// A repository with one commit: `a.txt` committed, then changed and staged.
fn repo() -> Computer {
    let mut c = machine();
    for line in [
        "cd /home/user/proj && git init",
        "cd /home/user/proj && git add a.txt sub/b.txt",
        "cd /home/user/proj && git commit -m first",
    ] {
        let r = run(&mut c, line);
        assert_eq!(r.exit_code, 0, "{line}: {}", r.stderr);
    }
    c
}

#[test]
fn git_reset_moves_the_index_the_branch_and_the_worktree() {
    let mut c = repo();
    // Stage a change, then unstage it by path: the worktree keeps it.
    ok(
        &mut c,
        "cd /home/user/proj && printf 'alpha\\nbeta\\ndelta\\n' > a.txt",
    );
    ok(&mut c, "cd /home/user/proj && git add a.txt");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\nM  a.txt\n"
    );
    let out = ok(&mut c, "cd /home/user/proj && git reset HEAD -- a.txt");
    assert!(
        out.contains("Unstaged changes after reset:\nM\ta.txt"),
        "{out:?}"
    );
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n M a.txt\n"
    );
    assert!(ok(&mut c, "cd /home/user/proj && cat a.txt").contains("delta"));
    // A second commit, then reset --soft: the branch moves, the index does not.
    ok(
        &mut c,
        "cd /home/user/proj && git add a.txt && git commit -m second",
    );
    ok(&mut c, "cd /home/user/proj && git reset --soft HEAD~1");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\nM  a.txt\n"
    );
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git log")
            .matches("commit ")
            .count(),
        1
    );
    // --mixed (the default) also resets the index; the file on disk is untouched.
    ok(&mut c, "cd /home/user/proj && git reset");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n M a.txt\n"
    );
    assert!(ok(&mut c, "cd /home/user/proj && cat a.txt").contains("delta"));
    // --hard throws the change away.
    ok(&mut c, "cd /home/user/proj && git reset --hard");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n"
    );
    assert!(ok(&mut c, "cd /home/user/proj && cat a.txt").contains("gamma"));
    // A hard reset with paths is refused, as git refuses it.
    let r = run(&mut c, "cd /home/user/proj && git reset --hard -- a.txt");
    assert_ne!(r.exit_code, 0);
    assert!(r.stderr.contains("hard reset with paths"), "{}", r.stderr);
}

#[test]
fn git_restore_puts_back_the_worktree_and_the_index() {
    let mut c = repo();
    ok(
        &mut c,
        "cd /home/user/proj && echo changed > a.txt && echo more > sub/b.txt",
    );
    // Discard Changes on one file only.
    ok(&mut c, "cd /home/user/proj && git restore a.txt");
    assert!(ok(&mut c, "cd /home/user/proj && cat a.txt").contains("gamma"));
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n M sub/b.txt\n"
    );
    // A folder stands for everything in it.
    ok(&mut c, "cd /home/user/proj && git restore sub");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n"
    );
    // Unstage with --staged, keeping the working copy.
    ok(
        &mut c,
        "cd /home/user/proj && echo staged > a.txt && git add a.txt",
    );
    ok(&mut c, "cd /home/user/proj && git restore --staged a.txt");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n M a.txt\n"
    );
    assert!(ok(&mut c, "cd /home/user/proj && cat a.txt").contains("staged"));
    // Both at once puts the file back to HEAD; a new file is untracked again.
    ok(
        &mut c,
        "cd /home/user/proj && git restore --staged --worktree a.txt",
    );
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n"
    );
    ok(
        &mut c,
        "cd /home/user/proj && echo new > c.txt && git add c.txt",
    );
    ok(&mut c, "cd /home/user/proj && git restore --staged c.txt");
    assert_eq!(
        ok(&mut c, "cd /home/user/proj && git status"),
        "On branch main\n A c.txt\n"
    );
    // `git checkout -- <path>` is the same thing.
    ok(&mut c, "cd /home/user/proj && echo again > a.txt");
    ok(&mut c, "cd /home/user/proj && git checkout -- a.txt");
    assert!(ok(&mut c, "cd /home/user/proj && cat a.txt").contains("gamma"));
    // Restoring nothing in particular is refused rather than guessed at.
    let r = run(&mut c, "cd /home/user/proj && git restore");
    assert_ne!(r.exit_code, 0);
    assert!(r.stderr.contains("specify path"), "{}", r.stderr);
}

#[test]
fn git_diff_separates_the_index_from_the_worktree() {
    let mut c = repo();
    ok(
        &mut c,
        "cd /home/user/proj && echo staged > a.txt && git add a.txt",
    );
    ok(&mut c, "cd /home/user/proj && echo working > a.txt");
    let staged = ok(&mut c, "cd /home/user/proj && git diff --staged");
    assert!(
        staged.contains("--- a/a.txt") && staged.contains("+staged"),
        "{staged:?}"
    );
    assert!(!staged.contains("+working"));
    let cached = ok(&mut c, "cd /home/user/proj && git diff --cached");
    assert_eq!(cached, staged);
    let unstaged = ok(&mut c, "cd /home/user/proj && git diff");
    assert!(
        unstaged.contains("-staged") && unstaged.contains("+working"),
        "{unstaged:?}"
    );
}

// ---------------------------------------------------------------- awk

/// A scratch machine with the files the awk and text-utility rows use.
fn textbox() -> Computer {
    let mut c = machine();
    for setup in [
        "printf 'alpha 1\\nbeta 2\\ngamma 3\\n' > /tmp/rows",
        "printf 'a,b,c\\n1,2,3\\n' > /tmp/csv",
        "printf 'a\\nb\\nc\\n' > /tmp/abc",
        "printf 'a\\nx\\nc\\n' > /tmp/axc",
    ] {
        let r = run(&mut c, setup);
        assert_eq!(r.exit_code, 0, "{setup}: {}", r.stderr);
    }
    c
}

#[test]
fn awk_runs_patterns_expressions_and_ranges() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "awk '{print $1}' /tmp/rows"),
        "alpha\nbeta\ngamma\n"
    );
    assert_eq!(ok(&mut c, "awk '/beta/' /tmp/rows"), "beta 2\n");
    assert_eq!(
        ok(&mut c, "awk '$2 > 1 {print $1}' /tmp/rows"),
        "beta\ngamma\n"
    );
    assert_eq!(
        ok(&mut c, "awk '/alpha/,/beta/{print NR}' /tmp/rows"),
        "1\n2\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{print \"start\"} {n++} END{print n}' /tmp/rows"
        ),
        "start\n3\n"
    );
    // A rule with no action prints the record; BEGIN alone never reads input.
    assert_eq!(ok(&mut c, "awk 'BEGIN{print 1}' /tmp/rows"), "1\n");
    assert_eq!(ok(&mut c, "awk 'NR==2' /tmp/rows"), "beta 2\n");
}

#[test]
fn awk_fields_rebuild_the_record() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "awk '{print NF, $NF}' /tmp/rows"),
        "2 1\n2 2\n2 3\n"
    );
    assert_eq!(
        ok(&mut c, "awk '{$1=\"X\"; print}' /tmp/rows"),
        "X 1\nX 2\nX 3\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{OFS=\"-\"} {$1=$1; print}' /tmp/rows"),
        "alpha-1\nbeta-2\ngamma-3\n"
    );
    // Assigning past NF pads with empty fields; assigning NF truncates.
    assert_eq!(
        ok(&mut c, "awk 'NR==1{$4=\"z\"; print NF; print}' /tmp/rows"),
        "4\nalpha 1  z\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'NR==1{NF=1; print $0}' /tmp/rows"),
        "alpha\n"
    );
    assert_eq!(ok(&mut c, "awk -F, '{print $2}' /tmp/csv"), "b\n2\n");
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{FS=\",\"} NR==2{print $3}' /tmp/csv"),
        "3\n"
    );
    assert_eq!(
        ok(&mut c, "awk '{print FILENAME, FNR}' /tmp/csv"),
        "/tmp/csv 1\n/tmp/csv 2\n"
    );
}

#[test]
fn awk_has_control_flow_arrays_and_functions() {
    let mut c = textbox();
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{for(i=1;i<=3;i++){if(i==2) continue; print i}}'"
        ),
        "1\n3\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{i=0; while(i<2){print i; i++}}'"),
        "0\n1\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{i=0; do{print i; i++}while(i<2)}'"),
        "0\n1\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{for(i=0;;i++){if(i>1) break}; print i}'"),
        "2\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{a[\"x\"]=1; a[\"y\"]=2; for(k in a) print k, a[k]}'"
        ),
        "x 1\ny 2\n"
    );
    // Multidimensional subscripts join on SUBSEP, and `delete` really deletes.
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{a[1,2]=7; for(k in a){split(k,p,SUBSEP); print p[1],p[2],a[k]}}'"
        ),
        "1 2 7\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{a[1]=1;a[2]=2; delete a[1]; n=0; for(k in a)n++; print n; delete a; m=0; for(k in a)m++; print m}'"),
        "1\n0\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{print (1 in a), (a[1]==\"\"), (1 in a)}'"
        ),
        "0 1 1\n"
    );
    // User functions, with the extra parameters acting as locals.
    assert_eq!(
        ok(
            &mut c,
            "awk 'function add(a,b,   t){t=a+b; return t} BEGIN{print add(2,3); print t \"|\"}'"
        ),
        "5\n|\n"
    );
    // Arrays are passed by reference.
    assert_eq!(
        ok(
            &mut c,
            "awk 'function fill(arr){arr[\"k\"]=9} BEGIN{fill(x); print x[\"k\"]}'"
        ),
        "9\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk '{next; print \"never\"} END{print NR}' /tmp/rows"
        ),
        "3\n"
    );
    let r = run(&mut c, "awk 'BEGIN{exit 4}'");
    assert_eq!(r.exit_code, 4);
}

#[test]
fn awk_string_and_math_library_is_complete() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{s=\"hello\"; print length(s), substr(s,2,3), index(s,\"ll\"), toupper(s), tolower(\"AB\")}'"),
        "5 ell 3 HELLO ab\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{print substr(\"hello\",0,3), substr(\"hello\",4)}'"
        ),
        "he lo\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{n=split(\"a:b:c\",p,\":\"); print n, p[1], p[3]}'"
        ),
        "3 a c\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{s=\"aaa\"; print gsub(/a/,\"b\",s), s}'"),
        "3 bbb\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{s=\"aaa\"; print sub(/a/,\"[&]\",s), s}'"
        ),
        "1 [a]aa\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{s=\"a\"; sub(/a/,\"\\\\&\",s); print s}'"
        ),
        "&\n"
    );
    // An empty match that touches the end of the previous one is not a match, so the
    // run of `l`s produces one dash, not two.
    assert_eq!(
        ok(
            &mut c,
            "printf 'hello\\n' | awk '{gsub(/l*/,\"-\"); print}'"
        ),
        "-h-e-o-\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{if(match(\"foobar\",/o+/)) print RSTART, RLENGTH; match(\"x\",/z/); print RSTART, RLENGTH}'"),
        "2 2\n0 -1\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print sprintf(\"%d-%s\", 7, \"x\")}'"),
        "7-x\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{print int(3.9), int(-3.9), sqrt(16), exp(0), log(1), atan2(0,1)}'"
        ),
        "3 -3 4 1 0 0\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{printf \"%.3f %.3f\\n\", sin(0), cos(0)}'"
        ),
        "0.000 1.000\n"
    );
    // rand is seeded from the world, so a replay is identical and bounded.
    let first = ok(&mut c, "awk 'BEGIN{srand(7); printf \"%.5f\\n\", rand()}'");
    let again = ok(&mut c, "awk 'BEGIN{srand(7); printf \"%.5f\\n\", rand()}'");
    assert_eq!(first, again, "a seeded rand must replay");
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{srand(1); x=srand(2); print x}'"),
        "1\n"
    );
}

#[test]
fn awk_printf_covers_the_conversion_set() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{printf \"%5.2f|%-5s|%05d|%x|%X|%o|%c|%e\\n\", 3.14159, \"ab\", 7, 255, 255, 8, 65, 1500}'"),
        " 3.14|ab   |00007|ff|FF|10|A|1.500000e+03\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{printf \"%s|%i|%%\\n\", \"x\", 42}'"),
        "x|42|%\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{printf \"%*d|%.*f\\n\", 5, 42, 2, 3.14159}'"
        ),
        "   42|3.14\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{printf \"%g %g\\n\", 0.0001, 1000000}'"),
        "0.0001 1e+06\n"
    );
    refused(&mut c, "awk 'BEGIN{printf \"%q\\n\", 1}'", "%q");
}

#[test]
fn awk_comparisons_follow_the_posix_rules() {
    let mut c = textbox();
    // An uninitialised value is both 0 and "".
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print (x==0), (x==\"\"), (x<1)}'"),
        "1 1 1\n"
    );
    // A field that looks numeric compares numerically; a quoted constant is a string.
    assert_eq!(
        ok(
            &mut c,
            "printf '10\\n9\\n' | awk '$1 > 9 {print \"num\", $1}'"
        ),
        "num 10\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print (\"10\" < \"9\"), (10 < 9)}'"),
        "1 0\n"
    );
    assert_eq!(ok(&mut c, "awk 'BEGIN{x=\"3\"; print (x==3)}'"), "1\n");
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print 1 \" \" 2, 1+1 \"x\"}'"),
        "1 2 2x\n"
    );
    assert_eq!(ok(&mut c, "awk 'BEGIN{print -2^2, 2^3^2}'"), "-4 512\n");
}

#[test]
fn awk_reads_and_writes_streams() {
    let mut c = textbox();
    // print > file, then close, then getline the file back.
    assert_eq!(
        ok(
            &mut c,
            "awk '{print $1 > \"/tmp/out1\"} END{close(\"/tmp/out1\"); while((getline l < \"/tmp/out1\")>0) print \"R:\" l}' /tmp/rows"
        ),
        "R:alpha\nR:beta\nR:gamma\n"
    );
    assert_eq!(ok(&mut c, "cat /tmp/out1"), "alpha\nbeta\ngamma\n");
    // A pipe runs when it closes; commands never run concurrently in this world.
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print \"z\" | \"sort\"; print \"a\" | \"sort\"; close(\"sort\"); print \"after\"}'"),
        "a\nz\nafter\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{\"echo piped\" | getline x; print \"got\", x}'"
        ),
        "got piped\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "awk 'BEGIN{while((\"printf \\\"a\\\\nb\\\\n\\\"\" | getline l) > 0) print \"L:\" l}'"
        ),
        "L:a\nL:b\n"
    );
    // A getline that cannot open its file answers -1 rather than aborting.
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print (getline x < \"/nope\")}'"),
        "-1\n"
    );
    // Plain getline advances the main input.
    assert_eq!(
        ok(&mut c, "awk 'NR==1{getline; print \"then\", $0}' /tmp/rows"),
        "then beta 2\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print system(\"echo ran\")}'"),
        "ran\n0\n"
    );
    assert_eq!(ok(&mut c, "awk 'BEGIN{print \"x\" >> \"/tmp/app\"; close(\"/tmp/app\")} END{}' /dev/null; cat /tmp/app"), "x\n");
}

#[test]
fn awk_variables_and_separators_are_settable() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "awk -v n=5 'BEGIN{print n*2}'"), "10\n");
    assert_eq!(
        ok(&mut c, "awk -v s='a\\tb' 'BEGIN{print length(s)}'"),
        "3\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{ORS=\"|\"} {print $1}' /tmp/rows"),
        "alpha|beta|gamma|"
    );
    assert_eq!(
        ok(
            &mut c,
            "printf 'a\\n\\nb\\nc\\n' | awk 'BEGIN{RS=\"\"} {print NR \":\" NF}'"
        ),
        "1:1\n2:2\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "printf 'a;b;c' | awk 'BEGIN{RS=\";\"} {print NR, $0}'"
        ),
        "1 a\n2 b\n3 c\n"
    );
    assert_eq!(
        ok(&mut c, "awk 'BEGIN{print ENVIRON[\"HOME\"]}'"),
        "/home/user\n"
    );
    // Operand assignments happen when the operand is reached.
    assert_eq!(
        ok(&mut c, "awk '{print v, $1}' v=one /tmp/rows"),
        "one alpha\none beta\none gamma\n"
    );
    // -f loads a program file, and several -f files concatenate.
    ok(
        &mut c,
        "printf 'BEGIN{x=1}\\n' > /tmp/p1.awk; printf 'BEGIN{print x+1}\\n' > /tmp/p2.awk",
    );
    assert_eq!(ok(&mut c, "awk -f /tmp/p1.awk -f /tmp/p2.awk"), "2\n");
}

#[test]
fn awk_refuses_what_it_cannot_do() {
    let mut c = textbox();
    refused(&mut c, "awk -W foo 'BEGIN{}'", "invalid option -- 'W'");
    refused(
        &mut c,
        "awk --bogus 'BEGIN{}'",
        "unrecognized option '--bogus'",
    );
    refused(&mut c, "awk", "no program text");
    refused(&mut c, "awk 'BEGIN{'", "missing `}`");
    refused(&mut c, "awk 'BEGIN{print 1/0}'", "division by zero");
    refused(&mut c, "awk 'BEGIN{nosuch()}'", "undefined function");
    refused(&mut c, "awk -v bad 'BEGIN{}'", "NAME=VALUE");
    let r = run(&mut c, "awk '{print $1}' /nope");
    assert_eq!(
        (r.exit_code, r.stderr.trim()),
        (1, "awk: /nope: No such file or directory")
    );
}

// ---------------------------------------------------------------- sed

#[test]
fn sed_addresses_every_way_it_can() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "sed -n '2p' /tmp/rows"), "beta 2\n");
    assert_eq!(ok(&mut c, "sed -n '$p' /tmp/rows"), "gamma 3\n");
    assert_eq!(ok(&mut c, "sed -n '/beta/p' /tmp/rows"), "beta 2\n");
    assert_eq!(ok(&mut c, "sed -n '1,2p' /tmp/rows"), "alpha 1\nbeta 2\n");
    assert_eq!(ok(&mut c, "sed -n '1~2p' /tmp/rows"), "alpha 1\ngamma 3\n");
    assert_eq!(ok(&mut c, "sed -n '1,+1p' /tmp/rows"), "alpha 1\nbeta 2\n");
    assert_eq!(ok(&mut c, "sed -n '2!p' /tmp/rows"), "alpha 1\ngamma 3\n");
    assert_eq!(
        ok(&mut c, "sed -n '0,/beta/p' /tmp/rows"),
        "alpha 1\nbeta 2\n"
    );
    assert_eq!(
        ok(&mut c, "sed -n '/alpha/,/beta/p' /tmp/rows"),
        "alpha 1\nbeta 2\n"
    );
    assert_eq!(ok(&mut c, "sed -n '\\%beta%p' /tmp/rows"), "beta 2\n");
    assert_eq!(ok(&mut c, "sed -n '/BETA/Ip' /tmp/rows"), "beta 2\n");
    refused(&mut c, "sed '0p' /tmp/rows", "line address 0");
    refused(&mut c, "sed '1,' /tmp/rows", "second address");
}

#[test]
fn sed_substitutes_with_every_flag() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "sed 's/a/A/' /tmp/rows"),
        "Alpha 1\nbetA 2\ngAmma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's/a/A/g' /tmp/rows"),
        "AlphA 1\nbetA 2\ngAmmA 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's/a/A/2' /tmp/rows"),
        "alphA 1\nbeta 2\ngammA 3\n"
    );
    assert_eq!(ok(&mut c, "sed -n 's/alpha/X/p' /tmp/rows"), "X 1\n");
    assert_eq!(
        ok(&mut c, "sed 's/ALPHA/X/I' /tmp/rows"),
        "X 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's/\\(al\\)pha/[\\1]/' /tmp/rows"),
        "[al] 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed -E 's/(al)pha/[\\1]/' /tmp/rows"),
        "[al] 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's/alpha/<&>/' /tmp/rows"),
        "<alpha> 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's/alpha/\\U&/' /tmp/rows"),
        "ALPHA 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 's|a|A|' /tmp/rows"),
        "Alpha 1\nbetA 2\ngAmma 3\n"
    );
    ok(&mut c, "sed -n 's/a/A/gw /tmp/sw' /tmp/rows");
    assert_eq!(ok(&mut c, "cat /tmp/sw"), "AlphA 1\nbetA 2\ngAmmA 3\n");
    // BRE and ERE really differ: `a\+` repeats, `a+` is a literal plus.
    assert_eq!(ok(&mut c, "printf 'aab\\n' | sed 's/a\\+/X/'"), "Xb\n");
    assert_eq!(ok(&mut c, "printf 'a+b\\n' | sed 's/a+/X/'"), "Xb\n");
    assert_eq!(ok(&mut c, "printf 'aab\\n' | sed -E 's/a+/X/'"), "Xb\n");
    // The same empty-match rule as awk's: `s/a*/X/g` over `aaa` is one `X`.
    assert_eq!(ok(&mut c, "printf 'aaa\\n' | sed 's/a*/X/g'"), "X\n");
    assert_eq!(ok(&mut c, "printf 'abc\\n' | sed 's/x*/-/g'"), "-a-b-c-\n");
    refused(&mut c, "sed 's/a/b/z' /tmp/rows", "unexpected `z`");
    refused(&mut c, "sed 's/a/b/0' /tmp/rows", "may not be zero");
    refused(&mut c, "sed 's/\\(a\\)\\1/X/' /tmp/rows", "backreference");
}

#[test]
fn sed_edits_lines_with_the_whole_command_set() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "sed '2d' /tmp/rows"), "alpha 1\ngamma 3\n");
    assert_eq!(
        ok(&mut c, "sed '1a added' /tmp/rows"),
        "alpha 1\nadded\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed '1i added' /tmp/rows"),
        "added\nalpha 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed '2c changed' /tmp/rows"),
        "alpha 1\nchanged\ngamma 3\n"
    );
    assert_eq!(
        ok(&mut c, "sed 'y/abc/ABC/' /tmp/rows"),
        "AlphA 1\nBetA 2\ngAmmA 3\n"
    );
    assert_eq!(ok(&mut c, "sed -n '$=' /tmp/rows"), "3\n");
    assert_eq!(ok(&mut c, "sed -n '1l' /tmp/rows"), "alpha 1$\n");
    assert_eq!(
        ok(&mut c, "sed -n '1{h}; ${G;p}' /tmp/rows"),
        "gamma 3\nalpha 1\n"
    );
    assert_eq!(
        ok(&mut c, "sed -n '1h; 2H; ${x;p}' /tmp/rows"),
        "alpha 1\nbeta 2\n"
    );
    assert_eq!(ok(&mut c, "sed -n 'N;P;D' /tmp/rows"), "alpha 1\nbeta 2\n");
    assert_eq!(
        ok(&mut c, "sed ':a;N;$!ba;s/\\n/,/g' /tmp/rows"),
        "alpha 1,beta 2,gamma 3\n"
    );
    assert_eq!(ok(&mut c, "sed -n '1{s/a/A/;p}' /tmp/rows"), "Alpha 1\n");
    assert_eq!(
        ok(&mut c, "sed '1{s/zz/Z/;t end};s/^/> /;:end' /tmp/rows"),
        "> alpha 1\n> beta 2\n> gamma 3\n"
    );
    assert_eq!(ok(&mut c, "sed -n '1r /tmp/abc' /tmp/rows"), "a\nb\nc\n");
    assert_eq!(
        ok(&mut c, "sed -n '1w /tmp/first' /tmp/rows; cat /tmp/first"),
        "alpha 1\n"
    );
    assert_eq!(ok(&mut c, "sed '2Q' /tmp/rows"), "alpha 1\n");
    assert_eq!(ok(&mut c, "sed '1z' /tmp/rows"), "\nbeta 2\ngamma 3\n");
    let r = run(&mut c, "sed '2q5' /tmp/rows");
    assert_eq!((r.exit_code, r.stdout.as_str()), (5, "alpha 1\nbeta 2\n"));
    refused(&mut c, "sed 'Z' /tmp/rows", "unknown command `Z`");
    refused(&mut c, "sed '{p' /tmp/rows", "unmatched `{`");
    refused(&mut c, "sed 'b nowhere' /tmp/rows", "label");
}

#[test]
fn sed_scripts_come_from_every_source() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "sed -n -e 1p -e 3p /tmp/rows"),
        "alpha 1\ngamma 3\n"
    );
    ok(&mut c, "printf '1d\\n$d\\n' > /tmp/prog.sed");
    assert_eq!(ok(&mut c, "sed -f /tmp/prog.sed /tmp/rows"), "beta 2\n");
    ok(&mut c, "cp /tmp/rows /tmp/inplace");
    ok(&mut c, "sed -i 's/alpha/A/' /tmp/inplace");
    assert_eq!(ok(&mut c, "head -n 1 /tmp/inplace"), "A 1\n");
    ok(&mut c, "cp /tmp/rows /tmp/inplace2");
    ok(&mut c, "sed -i.bak 's/alpha/A/' /tmp/inplace2");
    assert_eq!(ok(&mut c, "head -n 1 /tmp/inplace2.bak"), "alpha 1\n");
    // Several files are one stream unless -s says otherwise.
    assert_eq!(ok(&mut c, "sed -n '$p' /tmp/abc /tmp/axc"), "c\n");
    assert_eq!(ok(&mut c, "sed -sn '$p' /tmp/abc /tmp/axc"), "c\nc\n");
    refused(&mut c, "sed -i 's/a/b/'", "in place");
    refused(&mut c, "sed", "no script specified");
    let r = run(&mut c, "sed 'p' /nope");
    assert_eq!(
        (r.exit_code, r.stderr.trim()),
        (1, "sed: /nope: No such file or directory")
    );
}

// ---------------------------------------------------------------- xargs

#[test]
fn xargs_builds_command_lines_deterministically() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "printf 'a\\nb\\n' | xargs echo"), "a b\n");
    assert_eq!(ok(&mut c, "printf 'a\\nb\\n' | xargs -n1 echo"), "a\nb\n");
    assert_eq!(
        ok(&mut c, "printf 'a\\nb\\n' | xargs -I{} echo [{}]"),
        "[a]\n[b]\n"
    );
    assert_eq!(ok(&mut c, "printf 'a\\0b\\0' | xargs -0 echo"), "a b\n");
    assert_eq!(ok(&mut c, "printf 'a:b:' | xargs -d: echo"), "a b\n");
    assert_eq!(ok(&mut c, "printf '' | xargs -r echo"), "");
    assert_eq!(ok(&mut c, "printf '' | xargs echo"), "\n");
    // Quoting groups words, so a filename with a space survives.
    assert_eq!(ok(&mut c, "printf \"'a b'\\n\" | xargs -n1 echo"), "a b\n");
    // -P is accepted and runs sequentially: the order is part of the contract.
    assert_eq!(
        ok(&mut c, "printf '1\\n2\\n3\\n' | xargs -P4 -n1 echo"),
        "1\n2\n3\n"
    );
    // The status vocabulary: 123 when a command failed, 127 when it was missing.
    let r = run(&mut c, "printf 'x\\n' | xargs false");
    assert_eq!(r.exit_code, 123);
    let r = run(&mut c, "printf 'x\\n' | xargs nosuchcommand");
    assert_eq!(r.exit_code, 127);
    let r = run(&mut c, "printf 'a\\n' | xargs -t echo");
    assert_eq!((r.stdout.as_str(), r.stderr.trim()), ("a\n", "echo a"));
    refused(&mut c, "xargs -Z echo", "invalid option -- 'Z'");
}

// ---------------------------------------------------------------- text utilities

#[test]
fn cut_slices_bytes_characters_and_fields() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "cut -d, -f2 /tmp/csv"), "b\n2\n");
    assert_eq!(ok(&mut c, "cut -d, -f1,3 /tmp/csv"), "a,c\n1,3\n");
    assert_eq!(ok(&mut c, "cut -d, -f2- /tmp/csv"), "b,c\n2,3\n");
    assert_eq!(ok(&mut c, "cut -c1-3 /tmp/rows"), "alp\nbet\ngam\n");
    assert_eq!(ok(&mut c, "cut -b1 /tmp/rows"), "a\nb\ng\n");
    assert_eq!(
        ok(&mut c, "cut -d, -f1 --complement /tmp/csv"),
        "b,c\n2,3\n"
    );
    assert_eq!(
        ok(&mut c, "cut -d, -f1,3 --output-delimiter=: /tmp/csv"),
        "a:c\n1:3\n"
    );
    // A line with no delimiter passes through unless -s drops it.
    assert_eq!(ok(&mut c, "printf 'nodelim\\n' | cut -d, -f1"), "nodelim\n");
    assert_eq!(ok(&mut c, "printf 'nodelim\\n' | cut -s -d, -f1"), "");
    refused(&mut c, "cut /tmp/rows", "exactly one of -b, -c or -f");
    refused(&mut c, "cut -f0 /tmp/csv", "numbered from 1");
    refused(&mut c, "cut -c3-1 /tmp/rows", "decreasing range");
}

#[test]
fn sort_orders_by_key_and_by_type() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "printf '2\\n10\\n1\\n' | sort"), "1\n10\n2\n");
    assert_eq!(ok(&mut c, "printf '2\\n10\\n1\\n' | sort -n"), "1\n2\n10\n");
    assert_eq!(
        ok(&mut c, "printf '2\\n10\\n1\\n' | sort -nr"),
        "10\n2\n1\n"
    );
    assert_eq!(
        ok(&mut c, "printf 'b 2\\na 10\\n' | sort -k2,2n"),
        "b 2\na 10\n"
    );
    assert_eq!(
        ok(&mut c, "printf 'b:2\\na:10\\n' | sort -t: -k2,2n"),
        "b:2\na:10\n"
    );
    assert_eq!(ok(&mut c, "printf 'B\\na\\n' | sort -f"), "a\nB\n");
    assert_eq!(
        ok(&mut c, "printf '1.10\\n1.9\\n' | sort -V"),
        "1.9\n1.10\n"
    );
    assert_eq!(ok(&mut c, "printf 'Feb\\nJan\\n' | sort -M"), "Jan\nFeb\n");
    assert_eq!(ok(&mut c, "printf '2K\\n1M\\n' | sort -h"), "2K\n1M\n");
    assert_eq!(ok(&mut c, "printf '1e3\\n5\\n' | sort -g"), "5\n1e3\n");
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | sort -u"), "a\nb\n");
    assert_eq!(ok(&mut c, "printf '  b\\na\\n' | sort -b"), "a\n  b\n");
    ok(&mut c, "printf 'b\\na\\n' | sort -o /tmp/sorted");
    assert_eq!(ok(&mut c, "cat /tmp/sorted"), "a\nb\n");
    assert_eq!(ok(&mut c, "printf 'a\\nb\\n' | sort -c"), "");
    let r = run(&mut c, "printf 'b\\na\\n' | sort -c");
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("disorder"), "{r:?}");
    refused(&mut c, "sort -k /tmp/rows", "invalid key specification");
    refused(&mut c, "sort -kZ /tmp/rows", "invalid key specification");
}

#[test]
fn uniq_collapses_runs_by_every_rule() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | uniq"), "a\nb\n");
    assert_eq!(
        ok(&mut c, "printf 'a\\na\\nb\\n' | uniq -c"),
        "      2 a\n      1 b\n"
    );
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | uniq -d"), "a\n");
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | uniq -D"), "a\na\n");
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | uniq -u"), "b\n");
    assert_eq!(ok(&mut c, "printf 'A\\na\\n' | uniq -i"), "A\n");
    assert_eq!(ok(&mut c, "printf 'x a\\ny a\\n' | uniq -f1"), "x a\n");
    assert_eq!(ok(&mut c, "printf 'xa\\nya\\n' | uniq -s1"), "xa\n");
    assert_eq!(ok(&mut c, "printf 'abc\\nabd\\n' | uniq -w2"), "abc\n");
}

#[test]
fn head_tail_and_wc_count_what_they_claim() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "head -n 1 /tmp/rows"), "alpha 1\n");
    assert_eq!(ok(&mut c, "head -1 /tmp/rows"), "alpha 1\n");
    assert_eq!(ok(&mut c, "head -c 5 /tmp/rows"), "alpha");
    assert_eq!(ok(&mut c, "head -n -1 /tmp/rows"), "alpha 1\nbeta 2\n");
    assert_eq!(ok(&mut c, "tail -n 1 /tmp/rows"), "gamma 3\n");
    assert_eq!(ok(&mut c, "tail -n +2 /tmp/rows"), "beta 2\ngamma 3\n");
    assert_eq!(ok(&mut c, "tail -c 8 /tmp/rows"), "gamma 3\n");
    assert_eq!(
        ok(&mut c, "head -n 1 -v /tmp/rows"),
        "==> /tmp/rows <==\nalpha 1\n"
    );
    assert_eq!(ok(&mut c, "head -q -n 1 /tmp/abc /tmp/axc"), "a\na\n");
    assert_eq!(ok(&mut c, "wc -l /tmp/rows"), "3 /tmp/rows\n");
    assert_eq!(ok(&mut c, "wc -w /tmp/rows"), "6 /tmp/rows\n");
    assert_eq!(ok(&mut c, "wc -c /tmp/rows"), "23 /tmp/rows\n");
    assert_eq!(ok(&mut c, "wc -m /tmp/rows"), "23 /tmp/rows\n");
    assert_eq!(ok(&mut c, "wc -L /tmp/rows"), "7 /tmp/rows\n");
    assert_eq!(ok(&mut c, "cat /tmp/rows | wc -l"), "3\n");
    assert_eq!(
        ok(&mut c, "wc -l /tmp/abc /tmp/axc"),
        "3 /tmp/abc\n3 /tmp/axc\n6 total\n"
    );
    refused(&mut c, "tail -f /tmp/rows", "follow");
}

#[test]
fn tr_translates_deletes_and_squeezes() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "printf 'hello' | tr 'a-z' 'A-Z'"), "HELLO");
    assert_eq!(
        ok(&mut c, "printf 'hello' | tr '[:lower:]' '[:upper:]'"),
        "HELLO"
    );
    assert_eq!(ok(&mut c, "printf 'hello' | tr -d 'l'"), "heo");
    assert_eq!(ok(&mut c, "printf 'aabbcc' | tr -s 'ab'"), "abcc");
    assert_eq!(ok(&mut c, "printf 'a1b2' | tr -d -c '0-9'"), "12");
    assert_eq!(ok(&mut c, "printf 'abc' | tr 'abc' 'x'"), "xxx");
    assert_eq!(ok(&mut c, "printf 'a b' | tr ' ' '\\n'"), "a\nb");
    refused(&mut c, "tr", "missing operand");
    refused(&mut c, "tr a b c", "extra operand");
    refused(&mut c, "printf x | tr '[:bogus:]' y", "character class");
}

#[test]
fn column_tools_paste_join_and_compare() {
    let mut c = textbox();
    ok(
        &mut c,
        "printf 'a\\nb\\n' > /tmp/c1; printf '1\\n2\\n' > /tmp/c2",
    );
    assert_eq!(ok(&mut c, "paste /tmp/c1 /tmp/c2"), "a\t1\nb\t2\n");
    assert_eq!(ok(&mut c, "paste -d, /tmp/c1 /tmp/c2"), "a,1\nb,2\n");
    assert_eq!(ok(&mut c, "paste -s -d, /tmp/c1"), "a,b\n");
    ok(
        &mut c,
        "printf '1 a\\n2 b\\n' > /tmp/j1; printf '1 x\\n3 y\\n' > /tmp/j2",
    );
    assert_eq!(ok(&mut c, "join /tmp/j1 /tmp/j2"), "1 a x\n");
    assert_eq!(ok(&mut c, "join -a1 /tmp/j1 /tmp/j2"), "1 a x\n2 b\n");
    assert_eq!(ok(&mut c, "join -v2 /tmp/j1 /tmp/j2"), "3 y\n");
    assert_eq!(ok(&mut c, "join -o 1.2,2.2 /tmp/j1 /tmp/j2"), "a x\n");
    ok(&mut c, "printf 'a\\nc\\nd\\n' > /tmp/acd");
    assert_eq!(ok(&mut c, "comm -12 /tmp/abc /tmp/acd"), "a\nc\n");
    assert_eq!(ok(&mut c, "comm -23 /tmp/abc /tmp/acd"), "b\n");
    assert_eq!(
        ok(&mut c, "comm /tmp/abc /tmp/acd"),
        "\t\ta\nb\n\t\tc\n\td\n"
    );
    refused(&mut c, "join /tmp/j1", "two file operands");
    refused(&mut c, "comm /tmp/abc", "two file operands");
}

#[test]
fn diff_reports_differences_and_classifies_them() {
    let mut c = textbox();
    assert_eq!(run(&mut c, "diff /tmp/abc /tmp/abc").exit_code, 0);
    let r = run(&mut c, "diff /tmp/abc /tmp/axc");
    assert_eq!(
        (r.exit_code, r.stdout.as_str()),
        (1, "2c2\n< b\n---\n> x\n")
    );
    let r = run(&mut c, "diff -u /tmp/abc /tmp/axc");
    assert_eq!(
        r.stdout,
        "--- /tmp/abc\n+++ /tmp/axc\n@@ -1,3 +1,3 @@\n a\n-b\n+x\n c\n"
    );
    // A one-line span prints as `N` and an empty one as `N,0`, exactly as GNU does.
    ok(
        &mut c,
        "printf 'a\\nb\\n' > /tmp/two; printf 'a\\n' > /tmp/one",
    );
    let r = run(&mut c, "diff -u /tmp/two /tmp/one");
    assert_eq!(
        r.stdout,
        "--- /tmp/two\n+++ /tmp/one\n@@ -1,2 +1 @@\n a\n-b\n"
    );
    let r = run(&mut c, "diff -q /tmp/abc /tmp/axc");
    assert_eq!(r.stdout, "Files /tmp/abc and /tmp/axc differ\n");
    assert_eq!(
        ok(&mut c, "diff -s /tmp/abc /tmp/abc"),
        "Files /tmp/abc and /tmp/abc are identical\n"
    );
    assert_eq!(
        run(
            &mut c,
            "printf 'A\\n' > /tmp/u1; printf 'a\\n' > /tmp/u2; diff -i /tmp/u1 /tmp/u2"
        )
        .exit_code,
        0
    );
    assert_eq!(
        run(
            &mut c,
            "printf 'a b\\n' > /tmp/w1; printf 'a  b\\n' > /tmp/w2; diff -b /tmp/w1 /tmp/w2"
        )
        .exit_code,
        0
    );
    // A missing operand is an error, which diff spends 2 on.
    let r = run(&mut c, "diff /tmp/abc /nope");
    assert_eq!(r.exit_code, 2, "{r:?}");
    // Directories compare entry by entry with -r.
    ok(
        &mut c,
        "mkdir -p /tmp/d1 /tmp/d2; echo one > /tmp/d1/f; echo two > /tmp/d2/f",
    );
    let r = run(&mut c, "diff -r -q /tmp/d1 /tmp/d2");
    assert_eq!(
        (r.exit_code, r.stdout.as_str()),
        (1, "Files /tmp/d1/f and /tmp/d2/f differ\n")
    );
}

#[test]
fn small_filters_number_wrap_and_reverse() {
    let mut c = textbox();
    assert_eq!(
        ok(&mut c, "nl /tmp/abc"),
        "     1\ta\n     2\tb\n     3\tc\n"
    );
    assert_eq!(ok(&mut c, "nl -w2 -s: /tmp/abc"), " 1:a\n 2:b\n 3:c\n");
    assert_eq!(
        ok(&mut c, "nl -ba -nrz -w3 /tmp/abc"),
        "001\ta\n002\tb\n003\tc\n"
    );
    assert_eq!(ok(&mut c, "printf 'ab\\n' | rev"), "ba\n");
    assert_eq!(ok(&mut c, "printf 'abcdef\\n' | fold -w2"), "ab\ncd\nef\n");
    assert_eq!(
        ok(&mut c, "printf 'aa bb cc\\n' | fold -s -w6"),
        "aa bb \ncc\n"
    );
    assert_eq!(ok(&mut c, "printf 'a\\tb\\n' | expand -t4"), "a   b\n");
    assert_eq!(ok(&mut c, "printf '    a\\n' | unexpand -t4"), "\ta\n");
    assert_eq!(ok(&mut c, "echo hi | tee /tmp/tee1"), "hi\n");
    assert_eq!(ok(&mut c, "cat /tmp/tee1"), "hi\n");
    assert_eq!(
        ok(&mut c, "echo more | tee -a /tmp/tee1; cat /tmp/tee1"),
        "more\nhi\nmore\n"
    );
    assert_eq!(ok(&mut c, "seq 3"), "1\n2\n3\n");
    assert_eq!(ok(&mut c, "seq 2 4"), "2\n3\n4\n");
    assert_eq!(ok(&mut c, "seq -s, 1 2 7"), "1,3,5,7\n");
    assert_eq!(ok(&mut c, "seq -w 8 11"), "08\n09\n10\n11\n");
    assert_eq!(ok(&mut c, "seq -f '%03d' 2"), "001\n002\n");
    assert_eq!(ok(&mut c, "yes | head -3"), "y\ny\ny\n");
    assert_eq!(ok(&mut c, "yes no | head -2"), "no\nno\n");
    // shuf is seeded from the world: the same tick gives the same permutation.
    let a = ok(&mut c, "seq 5 | shuf");
    let b = ok(&mut c, "seq 5 | shuf");
    assert_eq!(a, b, "shuf must replay");
    assert_eq!(ok(&mut c, "seq 5 | shuf -n 2").lines().count(), 2);
    refused(&mut c, "fold -w0 /tmp/abc", "invalid number of columns");
    refused(&mut c, "nl -bz /tmp/abc", "invalid body numbering style");
}

#[test]
fn path_tools_answer_about_names_and_links() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "basename /a/b/c.txt"), "c.txt\n");
    assert_eq!(ok(&mut c, "basename /a/b/c.txt .txt"), "c\n");
    assert_eq!(ok(&mut c, "basename -a /a/b /c/d"), "b\nd\n");
    assert_eq!(ok(&mut c, "basename -s .txt /a/c.txt /a/d.txt"), "c\nd\n");
    assert_eq!(ok(&mut c, "dirname /a/b/c.txt"), "/a/b\n");
    assert_eq!(ok(&mut c, "dirname c.txt"), ".\n");
    assert_eq!(ok(&mut c, "realpath /home/user/../user"), "/home/user\n");
    assert_eq!(
        ok(&mut c, "readlink /home/user/link"),
        "/home/user/proj/a.txt\n"
    );
    assert_eq!(
        ok(&mut c, "readlink -f /home/user/link"),
        "/home/user/proj/a.txt\n"
    );
    refused(&mut c, "basename", "missing operand");
    refused(&mut c, "dirname", "missing operand");
    let r = run(&mut c, "readlink /tmp/abc");
    assert_eq!(r.exit_code, 1);
}

#[test]
fn printf_formats_like_coreutils() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "printf '%s\\n' a b c"), "a\nb\nc\n");
    assert_eq!(
        ok(&mut c, "printf '%5.2f|%-4s|%03d\\n' 3.14159 ab 7"),
        " 3.14|ab  |007\n"
    );
    assert_eq!(
        ok(&mut c, "printf '%x %X %o %c\\n' 255 255 8 hi"),
        "ff FF 10 h\n"
    );
    assert_eq!(ok(&mut c, "printf 'a%%b\\n'"), "a%b\n");
    assert_eq!(ok(&mut c, "printf '%b\\n' 'a\\tb'"), "a\tb\n");
    assert_eq!(ok(&mut c, "printf 'no args\\n'"), "no args\n");
    assert_eq!(ok(&mut c, "printf '%d\\n' 12abc"), "12\n");
    refused(&mut c, "printf '%z\\n' 1", "%z");
    refused(&mut c, "printf", "missing operand");
}

#[test]
fn encodings_digests_and_dumps_are_exact() {
    let mut c = textbox();
    assert_eq!(ok(&mut c, "printf 'abc' | base64"), "YWJj\n");
    assert_eq!(ok(&mut c, "printf 'YWJj' | base64 -d"), "abc");
    assert_eq!(ok(&mut c, "printf 'abcd' | base64 -w0"), "YWJjZA==\n");
    assert_eq!(
        ok(&mut c, "printf 'abc' | md5sum"),
        "900150983cd24fb0d6963f7d28e17f72  -\n"
    );
    assert_eq!(
        ok(&mut c, "printf 'abc' | sha1sum"),
        "a9993e364706816aba3e25717850c26c9cd0d89d  -\n"
    );
    assert_eq!(
        ok(&mut c, "printf 'abc' | sha256sum"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  -\n"
    );
    ok(
        &mut c,
        "printf 'abc' > /tmp/h.txt; md5sum /tmp/h.txt > /tmp/h.md5",
    );
    assert_eq!(ok(&mut c, "md5sum -c /tmp/h.md5"), "/tmp/h.txt: OK\n");
    ok(&mut c, "printf 'xyz' > /tmp/h.txt");
    let r = run(&mut c, "md5sum -c /tmp/h.md5");
    assert_eq!(r.exit_code, 1);
    assert!(r.stdout.contains("FAILED"), "{r:?}");
    assert_eq!(
        ok(&mut c, "printf 'hi\\n' | xxd"),
        "00000000: 6869 0a                                  hi.\n"
    );
    assert_eq!(ok(&mut c, "printf 'hi\\n' | xxd -p"), "68690a\n");
    assert_eq!(ok(&mut c, "printf '68690a' | xxd -r -p"), "hi\n");
    assert_eq!(
        ok(&mut c, "printf 'hi\\n' | od -c"),
        "0000000   h   i  \\n\n0000003\n"
    );
    assert_eq!(ok(&mut c, "printf 'hi\\n' | od -An -c"), "   h   i  \\n\n");
    assert_eq!(
        ok(&mut c, "printf 'hi\\n' | hexdump -C"),
        "00000000  68 69 0a                                         |hi.|\n00000003\n"
    );
    assert_eq!(run(&mut c, "cmp /tmp/abc /tmp/abc").exit_code, 0);
    let r = run(&mut c, "cmp /tmp/abc /tmp/axc");
    assert_eq!(
        (r.exit_code, r.stdout.as_str()),
        (1, "/tmp/abc /tmp/axc differ: byte 3, line 2\n")
    );
    assert_eq!(run(&mut c, "cmp -s /tmp/abc /tmp/axc").exit_code, 1);
    refused(&mut c, "od -t z /tmp/abc", "-t");
    refused(&mut c, "od -A q /tmp/abc", "address radix");
}

#[test]
fn split_strings_and_file_read_real_bytes() {
    let mut c = textbox();
    ok(&mut c, "split -l1 /tmp/abc /tmp/part");
    assert_eq!(
        ok(&mut c, "cat /tmp/partaa /tmp/partab /tmp/partac"),
        "a\nb\nc\n"
    );
    ok(&mut c, "split -l2 -d /tmp/abc /tmp/num");
    assert_eq!(ok(&mut c, "cat /tmp/num00"), "a\nb\n");
    assert_eq!(
        ok(&mut c, "strings /tmp/rows"),
        "alpha 1\nbeta 2\ngamma 3\n"
    );
    assert_eq!(ok(&mut c, "printf 'ab\\0cdef\\0' | strings -n 3"), "cdef\n");
    assert_eq!(ok(&mut c, "file /tmp/rows"), "/tmp/rows: ASCII text\n");
    assert_eq!(ok(&mut c, "file /home/user"), "/home/user: directory\n");
    assert_eq!(
        ok(&mut c, "printf '' > /tmp/empty; file /tmp/empty"),
        "/tmp/empty: empty\n"
    );
    assert_eq!(ok(&mut c, "file -b /tmp/rows"), "ASCII text\n");
    assert_eq!(ok(&mut c, "file -i /tmp/rows"), "/tmp/rows: text/plain\n");
    assert_eq!(
        ok(&mut c, "file /home/user/link"),
        "/home/user/link: symbolic link to /home/user/proj/a.txt\n"
    );
    // The magic table knows the formats this world writes. Binary bytes are placed
    // straight into the VFS: the shell's own streams are text, so a `printf` could
    // not carry them intact.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0, 0, 0, 13]);
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&4u32.to_be_bytes());
    png.extend_from_slice(&3u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (path, bytes) in [
        ("/tmp/x.png", png.as_slice()),
        ("/tmp/x.db", b"SQLite format 3\x00rest".as_slice()),
        ("/tmp/x.zip", b"PK\x03\x04rest".as_slice()),
        ("/tmp/x.pdf", b"%PDF-1.4\nrest".as_slice()),
        ("/tmp/x.wav", b"RIFF\x00\x00\x00\x00WAVEfmt ".as_slice()),
        ("/tmp/x.bin", b"\x01\x02\x03\x04\xff".as_slice()),
        ("/tmp/x.jpg", b"\xff\xd8\xff\xe0rest".as_slice()),
    ] {
        c.vfs.write_as(path, bytes, "user", 0).unwrap();
    }
    assert_eq!(
        ok(&mut c, "file /tmp/x.png"),
        "/tmp/x.png: PNG image data, 4 x 3, 8-bit/color RGBA, non-interlaced\n"
    );
    assert_eq!(
        ok(&mut c, "file /tmp/x.db"),
        "/tmp/x.db: SQLite 3.x database\n"
    );
    assert_eq!(
        ok(&mut c, "file /tmp/x.zip"),
        "/tmp/x.zip: Zip archive data\n"
    );
    assert!(ok(&mut c, "file /tmp/x.pdf").contains("PDF document"));
    assert!(ok(&mut c, "file /tmp/x.wav").contains("WAVE audio"));
    assert!(ok(&mut c, "file /tmp/x.jpg").contains("JPEG image data"));
    assert_eq!(ok(&mut c, "file /tmp/x.bin"), "/tmp/x.bin: data\n");
    assert_eq!(ok(&mut c, "file -i /tmp/x.png"), "/tmp/x.png: image/png\n");
}

// ---------------------------------------------------------------- the error contract

#[test]
fn every_message_names_the_command_and_the_reason() {
    let mut c = textbox();
    // `cmd: subject: reason`, in GNU's wording, for the errors people port against.
    for (line, want) in [
        (
            "grep pattern /nope",
            "grep: /nope: No such file or directory",
        ),
        ("cat /nope", "cat: /nope: No such file or directory"),
        ("wc -l /nope", "wc: /nope: No such file or directory"),
        ("sort /nope", "sort: /nope: No such file or directory"),
        ("cut -f1 /nope", "cut: /nope: No such file or directory"),
        ("head /nope", "head: /nope: No such file or directory"),
        (
            "awk '{print}' /nope",
            "awk: /nope: No such file or directory",
        ),
        ("sed p /nope", "sed: /nope: No such file or directory"),
        ("wc -l /home/user", "wc: /home/user: Is a directory"),
        (
            "rm /home/user/proj",
            "rm: cannot remove '/home/user/proj': Is a directory",
        ),
        (
            "cp /home/user/proj /tmp/copy2",
            "cp: -r not specified; omitting directory '/home/user/proj'",
        ),
    ] {
        let r = run(&mut c, line);
        assert_eq!(r.stderr.trim(), want, "`{line}`");
        assert_ne!(r.exit_code, 0, "`{line}` printed an error and returned 0");
    }
}

#[test]
fn every_command_rejects_an_invented_flag() {
    let mut c = textbox();
    // The consumer's worst bug is a flag that is accepted and then ignored. Every
    // command that parses options must refuse one it has never heard of, by name.
    for name in [
        "awk",
        "base64",
        "basename",
        "cat",
        "chmod",
        "cmp",
        "comm",
        "cp",
        "cut",
        "df",
        "diff",
        "dirname",
        "du",
        "expand",
        "file",
        "fold",
        "grep",
        "head",
        "hexdump",
        "join",
        "ls",
        "md5sum",
        "mkdir",
        "nl",
        "od",
        "paste",
        "ps",
        "readlink",
        "realpath",
        "rev",
        "rm",
        "sed",
        "seq",
        "sha1sum",
        "sha256sum",
        "shuf",
        "sort",
        "split",
        "stat",
        "strings",
        "tail",
        "tee",
        "tr",
        "unexpand",
        "uniq",
        "wc",
        "which",
        "xargs",
        "xxd",
        "yes",
        // Not text processing, but the contract is the whole shell's.
        "clear",
        "curl",
        "date",
        "env",
        "find",
        "git",
        "ip",
        "kill",
        "ln",
        "mv",
        "nproc",
        "read",
        "sqlite3",
        "sudo",
        "systemctl",
        "touch",
        "uptime",
    ] {
        let r = run(&mut c, &format!("{name} --invented-flag"));
        assert_eq!(
            r.exit_code, 2,
            "`{name} --invented-flag` must be refused with 2, said {r:?}"
        );
        assert!(
            r.stderr.contains("invented-flag"),
            "`{name}` must name the flag it refused, said {:?}",
            r.stderr
        );
        // `yes` is the one command whose getopt only looks at long options, so a
        // short flag really is its operand, exactly as in coreutils.
        if name == "yes" {
            continue;
        }
        let r = run(&mut c, &format!("{name} -\u{51}"));
        assert!(
            r.exit_code != 0 && (r.stderr.contains("'Q'") || r.stderr.contains("-Q")),
            "`{name} -Q` must be refused by name, said {r:?}"
        );
    }
}

#[test]
fn cases_from_the_consumer_report() {
    // A consumer's agent filed these against an older build. Every one is run here
    // verbatim so the report can never be true again without this suite going red.
    let mut c = textbox();
    ok(
        &mut c,
        "mkdir -p /tmp/r/sub; echo hi > /tmp/r/a.txt; echo yo > /tmp/r/sub/b.txt",
    );
    // find really applies its predicates.
    assert_eq!(
        ok(&mut c, "find /tmp/r -name '*.txt'"),
        "/tmp/r/a.txt\n/tmp/r/sub/b.txt\n"
    );
    assert_eq!(ok(&mut c, "find /tmp/r -type d"), "/tmp/r\n/tmp/r/sub\n");
    assert_eq!(
        ok(&mut c, "find /tmp/r -maxdepth 1 -type f"),
        "/tmp/r/a.txt\n"
    );
    // grep counts and lists.
    assert_eq!(ok(&mut c, "grep -c hi /tmp/r/a.txt"), "1\n");
    assert_eq!(ok(&mut c, "grep -l hi /tmp/r/a.txt"), "/tmp/r/a.txt\n");
    // ls -l, date, du, df, which and stat all answer.
    assert!(ok(&mut c, "ls -l /tmp/r").starts_with("total "));
    assert_eq!(ok(&mut c, "date +%Y-%m-%d"), "2026-09-17\n");
    assert!(ok(&mut c, "du -sh /tmp/r").ends_with("/tmp/r\n"));
    assert!(ok(&mut c, "df -h").contains("Mounted on"));
    assert_eq!(ok(&mut c, "which grep"), "/usr/bin/grep\n");
    assert_eq!(ok(&mut c, "stat -c %s /tmp/r/a.txt"), "3\n");
    // Heredocs, loops, case, functions and subshells.
    assert_eq!(ok(&mut c, "cat <<EOF\nheredoc line\nEOF"), "heredoc line\n");
    assert_eq!(ok(&mut c, "for i in 1 2; do echo $i; done"), "1\n2\n");
    assert_eq!(
        ok(
            &mut c,
            "i=0; while [ $i -lt 2 ]; do echo $i; i=$((i+1)); done"
        ),
        "0\n1\n"
    );
    assert_eq!(
        ok(&mut c, "case abc in a*) echo matched;; esac"),
        "matched\n"
    );
    assert_eq!(ok(&mut c, "f() { echo \"in f $1\"; }; f x"), "in f x\n");
    assert_eq!(ok(&mut c, "( cd /tmp; pwd ); pwd"), "/tmp\n/home/user\n");
    // Truthful exit codes.
    assert_eq!(ok(&mut c, "false; echo $?"), "1\n");
    assert_eq!(ok(&mut c, "echo a | grep -q a; echo $?"), "0\n");
    // The rows this task adds: awk, the text utilities and the error contract.
    assert_eq!(ok(&mut c, "seq 1 3 | awk '{s+=$1} END {print s}'"), "6\n");
    assert_eq!(ok(&mut c, "echo 'x y' | awk '{print $2}'"), "y\n");
    assert_eq!(
        ok(&mut c, "awk -F: '{print $1}' /tmp/colon 2>/dev/null; printf 'a:b\\n' > /tmp/colon; awk -F: '{print $2}' /tmp/colon"),
        "b\n"
    );
    assert_eq!(ok(&mut c, "printf 'a\\nb\\n' | sed -n '$p'"), "b\n");
    assert_eq!(ok(&mut c, "echo abc | rev"), "cba\n");
    assert_eq!(ok(&mut c, "echo abc | tr a-z A-Z"), "ABC\n");
    assert_eq!(ok(&mut c, "ls /tmp/r | wc -l"), "2\n");
    assert_eq!(ok(&mut c, "true && echo yes || echo no"), "yes\n");
    // A missing file names itself, in coreutils' wording, with a non-zero status.
    let r = run(&mut c, "ls /nope");
    assert_eq!(
        (r.exit_code, r.stderr.trim()),
        (1, "ls: cannot access '/nope': No such file or directory")
    );
}

#[test]
fn flags_that_are_accepted_and_inert_say_why_they_are() {
    // The matrix names four of these; each must succeed and change nothing, because a
    // silent no-op is only acceptable when it is published as one.
    let mut c = textbox();
    assert_eq!(ok(&mut c, "ls -1 /tmp/abc"), ok(&mut c, "ls /tmp/abc"));
    assert_eq!(
        ok(&mut c, "printf 'abc' | md5sum -b"),
        ok(&mut c, "printf 'abc' | md5sum -t")
    );
    assert_eq!(ok(&mut c, "echo hi | tee -i /tmp/inert"), "hi\n");
    assert_eq!(ok(&mut c, "strings -a -n1 /tmp/abc"), "a\nb\nc\n");
    assert_eq!(ok(&mut c, "nproc --all"), ok(&mut c, "nproc"));
    // And the ones that are not inert are refused instead.
    refused(&mut c, "ip -4 addr", "invalid option -- '4'");
    refused(&mut c, "tail -f /tmp/abc", "follow");
    refused(&mut c, "sort -z", "not modelled");
}

/// The published `find` matrix: every predicate row, exercised through the shell.
#[test]
fn find_applies_every_documented_predicate() {
    let mut c = machine();
    ok(&mut c, "mkdir -p /home/user/tree/a/b");
    ok(&mut c, "printf '0123456789' > /home/user/tree/ten.txt");
    ok(&mut c, "echo '' > /home/user/tree/a/one.md");
    ok(&mut c, "touch /home/user/tree/a/b/empty.txt");
    ok(&mut c, "chmod 750 /home/user/tree/a");
    ok(&mut c, "ln -s ../ten.txt /home/user/tree/a/alias");
    let lines = |out: String| {
        let mut v: Vec<String> = out.lines().map(String::from).collect();
        v.sort();
        v
    };
    assert_eq!(
        lines(ok(&mut c, "find /home/user/tree -name '*.txt'")),
        ["/home/user/tree/a/b/empty.txt", "/home/user/tree/ten.txt"]
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -iname 'TEN.TXT'"),
        "/home/user/tree/ten.txt\n"
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -path '*/a/b/*'"),
        "/home/user/tree/a/b/empty.txt\n"
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -regex '.*/ten[.]txt'"),
        "/home/user/tree/ten.txt\n"
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -type l"),
        "/home/user/tree/a/alias\n"
    );
    assert_eq!(
        lines(ok(&mut c, "find /home/user/tree -type d")),
        [
            "/home/user/tree",
            "/home/user/tree/a",
            "/home/user/tree/a/b"
        ]
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -type f -size +5c"),
        "/home/user/tree/ten.txt\n"
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -type f -size -1c"),
        "/home/user/tree/a/b/empty.txt\n"
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -perm 750"),
        "/home/user/tree/a\n"
    );
    assert_eq!(
        lines(ok(&mut c, "find /home/user/tree -type d -perm -0050")),
        [
            "/home/user/tree",
            "/home/user/tree/a",
            "/home/user/tree/a/b"
        ],
        "-MODE means every one of these bits"
    );
    assert_eq!(
        lines(ok(&mut c, "find /home/user/tree -type d -perm /0005")),
        ["/home/user/tree", "/home/user/tree/a/b"],
        "/MODE means any of these bits, and 0750 has none of them"
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -type f -empty"),
        "/home/user/tree/a/b/empty.txt\n"
    );
    assert_eq!(
        lines(ok(
            &mut c,
            "find /home/user/tree -user user -maxdepth 1 -mindepth 1"
        )),
        ["/home/user/tree/a", "/home/user/tree/ten.txt"]
    );
    assert_eq!(
        ok(&mut c, "find /home/user/tree -group user -name ten.txt"),
        "/home/user/tree/ten.txt\n"
    );
    // Time predicates measure back from the world clock.
    assert_eq!(
        ok(&mut c, "find /home/user/tree -name ten.txt -mmin -1"),
        "/home/user/tree/ten.txt\n"
    );
    // Tick 0 *is* the epoch, so an older stamp cannot exist; move forward instead.
    ok(
        &mut c,
        "touch -d 2026-09-20T09:00:00 /home/user/tree/ten.txt",
    );
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -name ten.txt -newermt 2026-09-19"
        ),
        "/home/user/tree/ten.txt\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -name ten.txt -newermt 2026-09-21"
        ),
        ""
    );
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -name ten.txt -newer /home/user/tree/a/one.md"
        ),
        "/home/user/tree/ten.txt\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -name one.md -newer /home/user/tree/ten.txt"
        ),
        ""
    );
    // Operators, grouping and -prune.
    assert_eq!(
        lines(ok(
            &mut c,
            "find /home/user/tree \\( -name '*.md' -o -name '*.txt' \\) -a -type f"
        )),
        [
            "/home/user/tree/a/b/empty.txt",
            "/home/user/tree/a/one.md",
            "/home/user/tree/ten.txt"
        ]
    );
    assert_eq!(
        lines(ok(&mut c, "find /home/user/tree -type f ! -name '*.txt'")),
        ["/home/user/tree/a/one.md"]
    );
    let pruned = ok(
        &mut c,
        "find /home/user/tree -name b -prune -o -type f -print",
    );
    assert!(!pruned.contains("empty.txt"), "{pruned}");
    // Actions.
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -name ten.txt -printf '%f %s %y\\n'"
        ),
        "ten.txt 10 f\n"
    );
    assert!(ok(&mut c, "find /home/user/tree -name ten.txt -ls").contains("ten.txt"));
    assert_eq!(
        ok(&mut c, "find /home/user/tree -name ten.txt -print0"),
        "/home/user/tree/ten.txt\0"
    );
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -type f -name '*.md' -exec cat {} \\;"
        ),
        "\n"
    );
    assert_eq!(
        ok(
            &mut c,
            "find /home/user/tree -type f -name '*.txt' -exec echo {} +"
        )
        .lines()
        .count(),
        1
    );
    ok(&mut c, "find /home/user/tree/a/b -delete");
    assert_eq!(run(&mut c, "test -d /home/user/tree/a/b").exit_code, 1);
    // `-quit` is an action, so the implicit `-print` is not added: ask for it.
    assert_eq!(ok(&mut c, "find /home/user/tree -type f -quit"), "");
    assert_eq!(
        ok(&mut c, "find /home/user/tree -type f -print -quit")
            .lines()
            .count(),
        1
    );
    // Refusals stay loud.
    refused(&mut c, "find /home/user -bogus", "-bogus");
    refused(&mut c, "find /home/user -name", "missing argument");
    refused(&mut c, "find /home/user -type s", "-type");
    refused(&mut c, "find /home/user -printf '%Q'", "%Q");
    assert_eq!(run(&mut c, "find /no/such/root").exit_code, 1);
}

/// Archives carry real format bytes, and a round trip returns the same tree.
#[test]
fn archives_round_trip_through_real_container_bytes() {
    let mut c = machine();
    ok(&mut c, "mkdir -p /tmp/src/sub");
    ok(&mut c, "echo alpha > /tmp/src/a.txt");
    ok(&mut c, "echo beta > /tmp/src/sub/b.txt");
    ok(&mut c, "chmod 750 /tmp/src/a.txt");
    ok(&mut c, "ln -s a.txt /tmp/src/alias");
    // tar: an extract restores bytes, mode and links.
    ok(&mut c, "tar -cf /tmp/src.tar -C /tmp src");
    assert!(ok(&mut c, "tar -tf /tmp/src.tar").contains("src/sub/b.txt"));
    ok(
        &mut c,
        "mkdir -p /tmp/out; tar -xf /tmp/src.tar -C /tmp/out",
    );
    assert_eq!(ok(&mut c, "cat /tmp/out/src/sub/b.txt"), "beta\n");
    assert_eq!(ok(&mut c, "stat -c %a /tmp/out/src/a.txt"), "750\n");
    assert_eq!(ok(&mut c, "readlink /tmp/out/src/alias"), "a.txt\n");
    // --strip-components drops leading path elements.
    ok(
        &mut c,
        "mkdir -p /tmp/flat; tar -xf /tmp/src.tar -C /tmp/flat --strip-components=1",
    );
    assert_eq!(ok(&mut c, "cat /tmp/flat/a.txt"), "alpha\n");
    // -z goes through a real gzip member.
    ok(&mut c, "tar -czf /tmp/src.tgz -C /tmp src");
    ok(&mut c, "mkdir -p /tmp/gz; tar -xzf /tmp/src.tgz -C /tmp/gz");
    assert_eq!(ok(&mut c, "cat /tmp/gz/src/a.txt"), "alpha\n");
    // gzip / gunzip / zcat.
    ok(&mut c, "echo payload > /tmp/p.txt");
    ok(&mut c, "gzip -k /tmp/p.txt");
    assert_eq!(ok(&mut c, "zcat /tmp/p.txt.gz"), "payload\n");
    ok(&mut c, "rm /tmp/p.txt; gunzip /tmp/p.txt.gz");
    assert_eq!(ok(&mut c, "cat /tmp/p.txt"), "payload\n");
    // zip / unzip.
    ok(&mut c, "zip -r /tmp/src.zip /tmp/src");
    let listing = ok(&mut c, "unzip -l /tmp/src.zip");
    assert!(listing.contains("a.txt"), "{listing}");
    ok(
        &mut c,
        "mkdir -p /tmp/unz; unzip -o -d /tmp/unz /tmp/src.zip",
    );
    assert_eq!(run(&mut c, "test -e /tmp/unz/tmp/src/a.txt").exit_code, 0);
    // rsync, local only.
    ok(&mut c, "mkdir -p /tmp/dst");
    ok(&mut c, "rsync -a /tmp/src/ /tmp/dst");
    assert_eq!(ok(&mut c, "cat /tmp/dst/sub/b.txt"), "beta\n");
    ok(&mut c, "echo stale > /tmp/dst/stale.txt");
    ok(&mut c, "rsync -a --delete /tmp/src/ /tmp/dst");
    assert_eq!(run(&mut c, "test -e /tmp/dst/stale.txt").exit_code, 1);
    ok(&mut c, "echo again > /tmp/dst/again.txt");
    ok(&mut c, "rsync -an --delete /tmp/src/ /tmp/dst");
    assert_eq!(run(&mut c, "test -e /tmp/dst/again.txt").exit_code, 0);
    refused(&mut c, "rsync -a /tmp/src/ host:/tmp/dst", "host:");
    refused(&mut c, "tar -cjf /tmp/x.tbz /tmp/src", "-j");
    refused(&mut c, "gzip --bogus /tmp/p.txt", "--bogus");
    // Compressed bytes cannot survive a text stdout, so that is refused by name.
    refused(&mut c, "gzip -c /tmp/p.txt", "stdout");
    refused(&mut c, "echo hi | gzip", "stdout");
    refused(&mut c, "unzip --bogus /tmp/src.zip", "--bogus");
}

/// The trash, from the shell: delete by mistake, then get it back.
#[test]
fn the_trash_keeps_a_restorable_record() {
    let mut c = machine();
    ok(&mut c, "trash /home/user/proj/a.txt");
    assert_eq!(run(&mut c, "test -e /home/user/proj/a.txt").exit_code, 1);
    assert_eq!(
        ok(
            &mut c,
            "cat /home/user/.local/share/Trash/info/a.txt.trashinfo"
        ),
        "[Trash Info]\nPath=/home/user/proj/a.txt\nDeletionDate=2026-09-17T09:00:00\n"
    );
    assert_eq!(
        ok(&mut c, "trash-list"),
        "2026-09-17 09:00:00 /home/user/proj/a.txt\n"
    );
    assert_eq!(
        ok(&mut c, "trash-restore /home/user/proj/a.txt"),
        "restored '/home/user/proj/a.txt'\n"
    );
    assert_eq!(
        ok(&mut c, "cat /home/user/proj/a.txt"),
        "alpha\nbeta\ngamma\n"
    );
    ok(&mut c, "trash /home/user/proj/a.txt");
    ok(&mut c, "trash-empty");
    assert_eq!(ok(&mut c, "trash-list"), "");
    refused(&mut c, "trash-empty 30", "age operand");
    // `gio` is two commands: the trash takes `gio trash`, the opener takes the rest.
    refused(&mut c, "gio mount /tmp", "gio open");
}
