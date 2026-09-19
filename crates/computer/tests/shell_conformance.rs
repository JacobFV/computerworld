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
/// An unsupported flag must be refused with status 2 and name itself.
fn refused(c: &mut Computer, line: &str, mention: &str) {
    let r = run(c, line);
    assert_eq!(r.exit_code, 2, "`{line}` must be refused, not ignored");
    assert!(
        r.stderr.contains(mention),
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
    assert_eq!(ok(&mut c, "wc -l /tmp/e"), "2\n");
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
    assert_eq!(ok(&mut c, "wc -l /tmp/all"), "2\n");
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
    refused(&mut c, "ls -Q /home/user", "-Q");
    refused(&mut c, "ls --color", "--color");
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
    refused(&mut c, "stat -f /home/user", "-f");
    refused(&mut c, "stat -c %Q /home/user", "%Q");
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
    refused(&mut c, "sed 'Z' /home/user/proj/a.txt", "supported scripts");
    refused(&mut c, "sed -r 's/a/b/' /home/user/proj/a.txt", "-r");
    refused(&mut c, "sed -e 1p -e 2p /home/user/proj/a.txt", "-e");
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
    refused(&mut c, "du -x /home/user", "-x");
    // df: fixed capacity, modelled usage, one filesystem.
    let df = ok(&mut c, "df");
    assert!(df.starts_with("Filesystem"), "{df}");
    assert!(df.contains("/dev/vda1"), "{df}");
    assert!(df.trim_end().ends_with(" /"), "{df}");
    assert_eq!(df.lines().count(), 2, "one modelled filesystem");
    assert!(ok(&mut c, "df -h").contains("64G"));
    assert!(ok(&mut c, "df -hT").contains("ext4"));
    assert_eq!(run(&mut c, "df /nope").exit_code, 1);
    refused(&mut c, "df -i", "-i");
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
    refused(&mut c, "uptime -h", "-h");
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
    refused(&mut c, "which -s grep", "-s");
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
    assert_eq!(ok(&mut c, "wc -l /home/user/proj/a.txt"), "3\n");
    assert_eq!(ok(&mut c, "wc -w /home/user/proj/a.txt"), "3\n");
    assert_eq!(ok(&mut c, "wc /home/user/proj/a.txt"), "3 3 17\n");
    assert_eq!(ok(&mut c, "head -n 1 /home/user/proj/a.txt"), "alpha\n");
    assert_eq!(ok(&mut c, "tail -n 1 /home/user/proj/a.txt"), "gamma\n");
    assert_eq!(ok(&mut c, "printf '2\\n10\\n1\\n' | sort"), "1\n10\n2\n");
    assert_eq!(ok(&mut c, "printf '2\\n10\\n1\\n' | sort -n"), "1\n2\n10\n");
    assert_eq!(
        ok(&mut c, "printf 'a\\na\\nb\\n' | uniq -c"),
        "      2 a\n      1 b\n"
    );
    assert_eq!(ok(&mut c, "printf 'a\\na\\nb\\n' | sort -u"), "a\nb\n");
    refused(&mut c, "head -c 3 /home/user/proj/a.txt", "-c");
    refused(&mut c, "sort -k2 /home/user/proj/a.txt", "-k");
    refused(&mut c, "wc -L /home/user/proj/a.txt", "-L");
    refused(&mut c, "uniq -i", "-i");
    // File commands parse their flags rather than skipping anything dash-shaped.
    assert_eq!(run(&mut c, "mkdir /home/user/proj").exit_code, 1);
    assert_eq!(run(&mut c, "mkdir /tmp/a/b/c").exit_code, 1);
    assert_eq!(ok(&mut c, "mkdir -p /tmp/a/b/c; ls /tmp/a/b"), "c\n");
    refused(&mut c, "mkdir -m 755 /tmp/x", "-m");
    refused(&mut c, "rm -i /home/user/proj/a.txt", "-i");
    refused(&mut c, "cp -a /home/user/proj /tmp/copy", "-a");
    refused(&mut c, "touch -t 1 /tmp/x", "-t");
    refused(&mut c, "touch --time=access /tmp/x", "--time");
    assert_eq!(run(&mut c, "cp /home/user/proj /tmp/copy").exit_code, 1);
    ok(&mut c, "cp -r /home/user/proj /tmp/copy");
    assert_eq!(ok(&mut c, "cat /tmp/copy/sub/b.txt"), "hi\n");
    // find already refuses unknown predicates; the matrix says so.
    refused(&mut c, "find /home/user -newer /tmp", "-newer");
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
    refused(&mut c, &format!("sed '0p' {file}"), "start at 1");
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
    refused(&mut c, "read -p prompt x", "-p");
    refused(&mut c, "read -t 5 x", "-t");
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
