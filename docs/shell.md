# Shell support matrix

The simulated shell in `crates/computer` is a bounded reimplementation, not a POSIX
shell. This page is the published surface: if a flag is not listed here it is not
supported, and invoking it **fails with a non-zero status** rather than being ignored.
`crates/computer/tests/shell_conformance.rs` runs every row below, so the table cannot
drift away from the code.

Two words are used throughout:

* **modelled** — real semantics computed from simulation state (the VFS, the process
  table, the environment, the tick). The answer changes when the world changes.
* **fixed** — a plausible constant read from `Computer::hardware`. The world does not
  simulate the underlying fact (cores, RAM, disk capacity, a NIC), so the value is
  stable per computer, identical on every call and every replay, and never sampled
  from the host. Change it by editing `Computer::hardware` before the episode runs.

## Exit codes

Every command returns a truthful status in `CommandResult::exit_code`. Classify by the
code; do not regex-match `stderr`.

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | A modelled negative or an operational error: no match, false test, missing file, permission denied. |
| `2` | Outside the simulated surface, or malformed: an unsupported flag, an unsupported `find` predicate, a `sed` script this world does not implement, a shell syntax error. |
| `126` | The command exists but cannot be executed (not executable, unsupported interpreter, package stub with no implementation). |
| `127` | No such command. |

This vocabulary is deliberately narrower than GNU coreutils, which spends `2` on
per-tool operational errors. Here `2` always means *you asked for something this world
does not implement* — the one distinction a porting consumer actually needs.

Nested shells (`sh -c`, `bash script`, executable scripts) and `git` propagate their
inner status rather than collapsing it to `1`, and a nested shell's `stderr` reaches the
caller even when it exits `0`, so a script never loses what it printed. `CommandResult` is serialised whole by
`terminal.v1 execute`, so the code reaches API consumers. The desktop terminal window
renders only `stdout + stderr`; GUI-driven agents read the code from the machine's
`terminal` state value instead.

## Grammar

| Construct | Status |
| --- | --- |
| `;` `\n` `&&` `\|\|` `\|` | modelled |
| `>` `>>` `<` | modelled |
| `2>` `2>>` `1>` `1>>` | modelled |
| `2>&1` `1>&2` `>&2` | modelled; descriptors are resolved left to right, so `>f 2>&1` and `2>&1 >f` differ as in bash |
| `&>` `&>>` | modelled |
| `/dev/null` as a redirect target | modelled: bytes are discarded and no file is created |
| `<<HEREDOC`, `<<-`, quoted delimiters | modelled |
| `'single'` `"double"` `\escape`, backticks | modelled |
| `$VAR` `${VAR}` `${VAR:-default}` `$?` `$0..$n` `$#` `${#}` `${#VAR}` `$@` `$*` | modelled |
| `$(command)` `` `command` `` `$((arithmetic))` | modelled, nesting bounded at 32 |
| `*` `?` globbing | modelled, one path component at a time; no `**`, no `[a-z]` classes |
| `NAME=value cmd` prefix assignments | modelled |
| trailing `&` | only `sleep N &` truly backgrounds; elsewhere `&` acts as a separator |
| `if … then … elif … else … fi` | modelled; the condition list's last status decides |
| `for NAME in WORDS; do … done`, `for NAME; do … done` | modelled; the second form walks `$1…$#` |
| `while` / `until … do … done` | modelled |
| `case WORD in PAT\|PAT) … ;; esac` | modelled; a leading `(` is accepted and patterns use the glob matcher, so `*` and `?` work and `[a-z]` does not |
| `NAME() { … }`, `function NAME { … }` | modelled; the body's output is the call's output, so a function pipes |
| `( … )` subshell | modelled; environment, working directory and function table are restored afterwards |
| `{ … ; }` group | modelled; runs in the caller's scope |
| `break [N]` `continue [N]` `return [N]` | modelled; outside a loop or a function they are **refused with status 2**, never a silent no-op |
| `(` `)` unquoted | metacharacters, as POSIX defines them: quote them to use them in a word |
| `[[ … ]]` | modelled, including `&&` `\|\|` `!` and parentheses inside it, `==`/`!=` pattern matching, `=~`, and `<`/`>` string order |
| redirection on a compound (`done < f`, `done > f`, `done 2>&1`) | modelled; `< f` becomes the shared input stream the body reads |
| a pipe through a compound (`cat f \| while … done`, `for … done \| wc -l`) | modelled |

### Word splitting

An unquoted expansion is split on whitespace and a quoted one is not, as in bash, so
`X='a b'; cmd $X` passes two arguments and `cmd "$X"` passes one. An unquoted expansion
that comes out empty contributes **no** argument at all, which is the difference
between `cmd $EMPTY` and `cmd "$EMPTY"`. A `*` that arrives from a variable stays
literal; only a `*` written in the source globs, so data never becomes a pattern.

### Standard input inside a compound

`< f` on a compound, and a pipe into one, give its body a single shared stream. `read`
takes one line from it and leaves the rest — that is what makes `while read line; do …;
done < f` advance and then stop. Any other command is handed the stream only if it
would actually read it (`cat`, `tr`, `cut`, and `grep`/`sed`/`head`/`tail`/`wc`/`sort`/
`uniq` when no operand names an existing file), and it consumes the whole of it. A loop
body that ignores stdin therefore cannot swallow the lines the loop is reading.

### Limits

A deterministic simulator must never hang, so every bound is a status, not a wait.
All four are per `execute` call and are refused with status `2`:

| Limit | Value | Message |
| --- | --- | --- |
| Source length | 64 KiB | `command exceeds 64 KiB limit` |
| Nesting depth (`$( )`, `sh -c`, scripts, `source`, subshells, function calls) | 32 | `execution nesting exceeds 32` |
| Simple commands run | 10 000 | `command budget exhausted after 10000 commands` |
| Loop iterations, summed over every loop | 10 000 | `loops exceeded 10000 total iterations` |

The step budget usually trips first, because every iteration of a loop runs at least
one command. A nested shell (`sh -c`, a script, a command substitution) starts a fresh
budget, so the worst case is bounded by depth × budget — finite, and identical on
every replay.

## Commands

| Command | Supported | Unsupported | Behaviour |
| --- | --- | --- | --- |
| `true` / `:` / `false` | — | — | modelled; `:` is the null command, so a loop body can do nothing |
| `echo` | `-n` | — | modelled |
| `printf` | `%s` `%d` `%%`, `\n` `\t` | width/precision, `%f`, `%x` | modelled |
| `pwd` | — | all flags | modelled |
| `whoami` | — | all flags | modelled |
| `hostname` | — | all flags | modelled (the computer id) |
| `uname` | — | `-a` `-r` `-m` and the rest | modelled (the OS family only) |
| `date` | `+FORMAT`, `-u`; `%Y %y %m %d %e %H %M %S %N %s %F %T %D %a %A %b %B %h %u %w %Z %z %%` | `-d` `-r` `-s`, any other conversion | modelled from the tick; see *Clock* below |
| `cd` | — | all flags | modelled |
| `env` / `printenv` | `NAME` | all flags | modelled |
| `export` / `unset` | — | all flags | modelled |
| `ls` / `dir` | `-a -A -l -h -d -F -r -t -S -R`, clusters (`-la`), `--all --almost-all --human-readable --reverse --recursive --directory --classify`; `-1` is accepted and is already the default | `-i` `-n` `-Q` `--color` and the rest | modelled; output is always one entry per line, and directories report one 4 KiB allocation unit as their size |
| `cat` | — | `-n` `-A` and the rest | modelled; no operand reads stdin |
| `touch` | `-c` / `--no-create`, `-d DATE` / `--date`, `-t STAMP`, `-r FILE` / `--reference`; `-a -m` accepted (the VFS keeps one timestamp) | `--time=`, relative dates (`yesterday`), timestamps before the epoch | modelled; creates missing files and sets the modification tick. `-d` takes `@SECONDS` or `YYYY-MM-DD[ HH:MM[:SS]]`, `-t` takes `[[CC]YY]MMDDhhmm[.ss]` |
| `mkdir` | `-p` / `--parents`; `-v` accepted and inert | `-m` | modelled; without `-p` an existing target or a missing parent is an error |
| `cp` | `-r` / `-R` / `--recursive`; `-f -p -v` accepted and inert | `-a` `-u` `-l` | modelled, including recursive directory copies |
| `mv` | — | all flags | modelled |
| `rm` / `rmdir` | `-r` / `-R` / `--recursive`, `-f` / `--force`, PowerShell `-Recurse` `-Force`; `-v` accepted and inert | `-i` | modelled; `rmdir` always removes recursively |
| `chmod` | octal mode, symbolic modes (`u+x`, `go-w`, `a=r`, `+X`, `u+s`, `+t`, comma lists), `-R` / `--recursive`; `-v` accepted and inert | `--reference`, copying permissions (`u=g`) | modelled; no umask is simulated, so a bare `+x` means `a+x`, and `X` reads the mode as the clauses before it left it |
| `ln` | `-s` | `-f` `-r` | modelled |
| `stat` | `-c FMT` / `--format=` / `--printf=`, `-L`; `%n %N %s %b %B %o %f %a %A %F %U %G %u %g %i %h %m %d %X %Y %Z %x %y %z %%` | `-f` `-t` `--cached`, any other conversion | modelled; see *Stat fidelity* below |
| `find` | `-name -iname -type f\|d -maxdepth` | every other predicate, refused by name | modelled |
| `grep` / `select-string` | `-i -v -n -c -l -L -F -E -q -s -h -H -w -x -r -R -e -o -A N -B N -C N`, long forms | `--include` `-P` `-m` | modelled; `0` matched, `1` did not. Context lines are prefixed with `-` where a matching line uses `:`, and `--` separates non-adjacent groups |
| `sed` | `-n -i -e`; addresses `N`, `$`, `/RE/` and any pair of them; commands `s/RE/REP/[g]`, `p`, `d`, `a TEXT`, `i TEXT`, `y/SET/SET/`, `q [CODE]` | `-r` `-E`, multiple `-e`, `b` `t` `n` `N` `w` `r`, the hold space, backreference addresses | modelled; a range opens on its first address and closes on the next line the second matches. `q CODE` becomes the exit status, and text after `a\\` keeps its leading blanks |
| `tr` | `-d`, ranges `a-z` | `-s` `-c` | modelled, stdin only |
| `cut` | `-d -f` | `-c` `-b` `--complement` | modelled, stdin only |
| `head` / `tail` | `-n N`, `-N` (bare count), `--lines` | `-c` `-f` `-q` | modelled |
| `wc` | `-l -w -c -m` | `-L` | modelled |
| `sort` | `-r -n -u` | `-k` `-t` `-f` `-h` | modelled |
| `uniq` | `-c -d -u` | `-i` `-f` | modelled |
| `du` | `-s -a -h -k -b -m -c -d N`, `--max-depth=`, `--summarize --all --human-readable --bytes --total` | `--exclude`, `-x`, `-L` | modelled over the VFS; block accounting assumes a 4 KiB allocation unit |
| `df` | `-h -k -T`, `--human-readable --print-type` | `-i` `-a` `-B` | **mixed**: capacity and device name are fixed, usage is summed from the VFS; one filesystem mounted at `/` |
| `which` | `-a`, `--all` | `-s` | modelled over `PATH`; see *which and builtins* below |
| `nproc` | `--all` | `--ignore` | **fixed** (`hardware.cpus`, default 4) |
| `uptime` | `-p -s`, `--pretty --since` | `-h` `-V` | modelled from the tick and `hardware.boot_tick`; the load average and user count are **fixed** at `0.00` and `1` |
| `clear` / `cls` | — | all arguments | modelled as a screen action: no output, `CommandResult::clear` is set |
| `ip` | `addr` \| `a` \| `address`, `link` \| `l`, `route` \| `r`, optional `show`/`list`; `-4 -6 -o` accepted and inert | every other object (`netns`, `tuntap`, `rule`, …) and every mutating action | **fixed** (`hardware.ipv4`, `prefix`, `mac`, `gateway`, `interface`) |
| `sudo` | `-u USER`, `--`, `-v -k -K` (succeed and do nothing); `-n -E` accepted and inert | password prompts, a sudoers policy, `-i`/`-s` login shells | modelled thinly: it swaps only the identity access checks use (default `root`, which bypasses VFS permissions). `HOME` and `USER` are left alone |
| `test` / `[` | `-e -f -d -s -r -w -x -L -h -n -z`, `= == != -eq -ne -lt -gt -le -ge -nt -ot` | `-a`/`-o`, `-p` `-S` `-g` `-u` `-k` | modelled against the VFS and its permissions |
| `[[ … ]]` | everything `test` takes, plus `&&` `\|\|` `!` and `( … )` inside the brackets, `==`/`!=` glob matching (an unquoted right side is a pattern, a quoted one a literal), `=~` regex, `<` `>` string order | `&&`-chaining onto other commands inside the brackets, `-v`, `-o` | modelled; a missing `]]` is a syntax error |
| `read` | `-r` (accepted; this shell never unescapes a read line), any number of names, the last taking the remainder; no name sets `REPLY` | `-p` `-t` `-n` `-d` `-s` `-u` `-a` | modelled; fields split on whitespace. Status `1` at end of input, and also when the last line had no terminating newline — the same end-of-file report bash gives |
| `exit` | `[N]` | — | modelled; ends this shell (or this `sh -c`, script or subshell) with `N`, or with the last status. It does not end the caller's shell |
| `shift` | `[N]` | — | modelled; status `1` when `N` exceeds `$#`, which changes nothing |
| `local` | `NAME[=VALUE]…` | `-r` `-i` `-a` | modelled; shadows the name until the enclosing function returns. Outside a function it is refused with status `2` |
| `source` / `.` | `FILE [ARG…]` | — | modelled; runs the file in this shell, so its variables, working directory and functions persist. `return` ends it, `exit` ends the whole shell, and it counts against the nesting limit |
| `getopts` | `OPTSTRING NAME [ARG…]`, clusters (`-ab`), glued and separate option arguments, a leading `:` for silent mode | `--long` options | modelled; `OPTIND` and `OPTARG` are ordinary shell variables, so resetting `OPTIND=1` restarts the scan |
| `ps` / `Get-Process` | `-e` / `-A`, `-f`, `-u USER`, `-p PID`, `--json` | `aux` and every other BSD operand, `-o` `-l` `--forest` | modelled; **column output by default**. `TTY` is `?` and `TIME` is `00:00:00` for every process because no terminal and no CPU accounting are simulated; a zombie prints `<defunct>`. With no selector `ps` lists the current user's processes. `--json` dumps the whole table as JSON — the pre-column behaviour, kept for machine consumers |
| `kill` | `-SIGNAL` / `-N` | `-l` | modelled against the process table and its signal dispositions |
| `sleep` | fractional seconds, trailing `&` | — | modelled against simulated time; never blocks the host |
| `systemctl` / `service` | `start stop restart status` | `enable` `disable` `daemon-reload` | modelled against the process table and the service adapter |
| `apt` / `apt-get` / `brew` / `winget` / `pip` / `npm` | `install`, `remove`/`uninstall`, `list` | `update` `upgrade` `search` | modelled against the package manager, offline |
| `curl` / `wget` | `-X -d --data --data-raw -H -o -f` | `-L` `-s` `-I` `-u` | modelled against the network adapter |
| `git` | see `crates/computer/src/git.rs` | — | modelled, content-addressed |
| `sh` / `bash` | `-c SCRIPT [NAME [ARG…]]`, script path plus arguments | `-e` `-x` | modelled; a nested run of the same shell, with its own budget and its own function table |
| `break` / `continue` / `return` | `[N]` | — | modelled as shell signals; see *Grammar* |
| PowerShell aliases | `Write-Output Get-Location Set-Location Get-ChildItem Get-Content Set-Content Add-Content Copy-Item Move-Item Remove-Item Select-String Get-Process Stop-Process Invoke-WebRequest Test-Path` | the rest of PowerShell | modelled; only available when the computer's dialect is `powershell` |
| anything else | — | — | status `127`, `command not found` |

## Clock

`date` is derived from the simulated tick, never the host clock. Tick 0 is
**09:00:00 UTC on Thursday 17 September 2026**, the same origin the desktop clock and
the GUI status bar use, so they never disagree. A tick is one microsecond. There is one
timezone (UTC) and no way to set the clock from the shell; advance simulated time
instead.

## Stat fidelity

The VFS stores one timestamp per node, so `%X` (access), `%Y` (modify) and `%Z`
(change) all report the modification tick, and `Birth:` prints `-`. There is no numeric
user database, so `%u`/`%g` report `hardware.uid`/`hardware.gid` (1000/1000 by default)
and `%G` repeats the owner name because groups are not modelled. Directory sizes are
reported as one 4 KiB allocation unit rather than the stored child count. `Device:` is
a constant.

## which and builtins

Everything in the table above runs in-process; the VFS holds no real binaries. `which`
first searches `PATH` in the VFS — so packages installed by `apt`/`pip` resolve to
their real installed path — and otherwise reports a nominal `/usr/bin/NAME` for any
command this shell implements. That keeps "is this available?" a truthful question.
`which` exits `1` only when *no* operand resolves.

## Fixed hardware facts

`Computer::hardware` holds every constant the probing commands report:

| Field | Default | Read by |
| --- | --- | --- |
| `cpus` | `4` | `nproc` |
| `memory_bytes` | 8 GiB | reserved |
| `disk_bytes` | 64 GiB | `df` |
| `device` | `/dev/vda1` | `df` |
| `interface` / `ipv4` / `prefix` / `mac` / `gateway` | `eth0` / `10.0.2.15` / `24` / `52:54:00:12:34:56` / `10.0.2.1` | `ip` |
| `boot_tick` | `0` | `uptime` |
| `uid` / `gid` | `1000` / `1000` | `stat` |

## Known gaps

Deliberately not implemented, and refused rather than faked:

* `trap` — it would need a signal-delivery model the process table does not have, and a
  handler that never fires is worse than a command that says it is missing.
* `eval` — re-entrant parsing of text built at runtime, for very little gain in a world
  where nothing arrives from outside the simulation. `sh -c` covers the honest cases.
* `select`, `export -f`, arrays, `declare`/`typeset`, `${VAR/…/…}` and `${VAR#…}`.
  Functions are not exported, so a command substitution, `sh -c` or a script starts with
  an empty function table, as it would without `export -f`.
* `[a-z]` character classes in globs, `case` patterns and `[[ == ]]` patterns; `**`.
* `"$@"` expands to one word joined by spaces rather than one word per parameter; use
  it unquoted to forward parameters that contain no spaces.
* Arithmetic beyond `+ - * / %` and parentheses: no `**`, no comparisons, no `++`.
* `sed`'s hold space, `b`/`t` branching, `N`/`n`, `w`/`r`, and more than one `-e`.
* `ps` columns that would have to be invented: `%CPU`, `%MEM`, `VSZ`, `RSS`, `STAT`.
  `ps aux` is refused by name rather than filled with plausible numbers.
* Real process scheduling: only `sleep` occupies simulated time.
* A group database, an allocator, and per-file access/change times: `touch -a` and
  `touch -m` both move the single stored timestamp.
