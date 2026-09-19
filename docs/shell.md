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

## The error contract

Every diagnostic this shell prints follows one shape, and every command that prints one
exits non-zero. The wording is GNU coreutils' own, because that is what scripts and
people match against:

| Shape | Example |
| --- | --- |
| `cmd: subject: reason` | `grep: nope: No such file or directory` |
| `cmd: action 'operand': reason` | `rm: cannot remove 'x': Is a directory` |
| `cmd: sentence` | `cp: -r not specified; omitting directory 'x'` |
| `cmd: invalid option -- 'x'` + `Usage: …` | a short flag the command does not have |
| `cmd: unrecognized option '--x'` + `Usage: …` | a long flag the command does not have |
| `cmd: option requires an argument -- 'x'` + `Usage: …` | a flag whose operand is missing |

The `reason` half is `strerror`'s wording, mapped from the VFS's own errors:

| VFS error | Reason |
| --- | --- |
| not found | `No such file or directory` |
| already exists | `File exists` |
| not a directory | `Not a directory` |
| is a directory | `Is a directory` |
| directory not empty | `Directory not empty` (`Is a directory` from `rm` without `-r`) |
| permission denied | `Permission denied` |
| symlink loop | `Too many levels of symbolic links` |
| invalid path | `Invalid argument` |

An unknown or unimplemented flag is **always** status `2` and **always** names itself.
`crates/computer/tests/shell_conformance.rs` runs every command in the table below with
an invented long flag and an invented short flag and asserts both are refused by name;
a flag that is accepted and then ignored is the one bug this suite exists to prevent.
Where a flag is accepted but does nothing, the table says so explicitly and gives the
reason — `xargs -P` (commands run sequentially so a replay is identical), `ls -1`
(output is already one entry per line), `md5sum -b`/`-t` (the VFS has no text/binary
distinction), `tee -i` (this world delivers no signals). `127` is reserved for a
command that does not exist; nothing else uses it.

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
would actually read it (`cat`, `tr`, `tee`, `xargs`, and `grep`/`sed`/`awk`/`cut`/
`head`/`tail`/`wc`/`sort`/`uniq`/`nl`/`rev`/`fold`/`expand`/`unexpand`/`paste`/`shuf`/
`split`/`strings`/`base64`/`md5sum`/`sha1sum`/`sha256sum`/`xxd`/`od`/`hexdump` when no
operand names an existing file), and it consumes the whole of it. A loop
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
| `printf` | the whole conversion set — `%s %b %c %d %i %o %u %x %X %e %E %f %F %g %G %%`, the flags `- + space 0 #`, width and precision including `*`; escapes `\n \t \r \a \b \f \v \e \\ \NNN \0NNN \xHH \c` | `%q`, `--help`/`--version` (the first operand is always the format, as POSIX requires) | modelled; the format is reused until the operands run out, so `printf '%s\n' a b c` prints three lines |
| `pwd` | — | all flags | modelled |
| `whoami` | — | all flags | modelled |
| `hostname` | — | all flags | modelled (the computer id) |
| `uname` | — | `-a` `-r` `-m` and the rest | modelled (the OS family only) |
| `date` | `+FORMAT`, `-u`; `%Y %y %m %d %e %H %M %S %N %s %F %T %D %a %A %b %B %h %u %w %Z %z %%` | `-d` `-r` `-s`, any other conversion | modelled from the tick; see *Clock* below |
| `cd` | — | all flags | modelled |
| `env` / `printenv` | `NAME` | all flags, and `env NAME=V CMD` (each would need a model this world does not have) | modelled |
| `export` / `unset` | `NAME=VALUE` / `NAME` | all flags, including `export -f` | modelled |
| `ls` / `dir` | `-a -A -l -h -d -F -r -t -S -R`, clusters (`-la`), `--all --almost-all --human-readable --reverse --recursive --directory --classify`; `-1` accepted and inert **because output is always one entry per line** | `-i` `-n` `-Q` `--color` and the rest | modelled; directories report one 4 KiB allocation unit as their size |
| `cat` | `-n -b -E -T -A -s -v`, `--number --number-nonblank --show-ends --show-tabs --show-all --squeeze-blank --show-nonprinting`; `-u` accepted and inert (output is never buffered) | every other flag, refused by name | modelled; no operand reads stdin, `-` names stdin |
| `touch` | `-c` / `--no-create`, `-d DATE` / `--date`, `-t STAMP`, `-r FILE` / `--reference`; `-a -m` accepted (the VFS keeps one timestamp) | `--time=`, relative dates (`yesterday`), timestamps before the epoch | modelled; creates missing files and sets the modification tick. `-d` takes `@SECONDS` or `YYYY-MM-DD[ HH:MM[:SS]]`, `-t` takes `[[CC]YY]MMDDhhmm[.ss]` |
| `mkdir` | `-p` / `--parents`, `-v` / `--verbose` | `-m` | modelled; without `-p` an existing target or a missing parent is an error. `-v` prints `mkdir: created directory 'x'` |
| `cp` | `-r` / `-R` / `--recursive`; `-f -p -v` accepted and inert | `-a` `-u` `-l` | modelled, including recursive directory copies |
| `mv` | `-f` (already the behaviour: nothing can prompt), `-v` / `--verbose`, PowerShell `-Force` | `-i` `-n` `-t` `-u` | modelled; moving onto a directory moves the name into it |
| `rm` / `rmdir` | `-r` / `-R` / `--recursive`, `-f` / `--force`, PowerShell `-Recurse` `-Force`; `-v` accepted and inert | `-i` | modelled; `rmdir` always removes recursively |
| `chmod` | octal mode, symbolic modes (`u+x`, `go-w`, `a=r`, `+X`, `u+s`, `+t`, comma lists), `-R` / `--recursive`, `-v` / `--verbose` | `--reference`, copying permissions (`u=g`) | modelled; no umask is simulated, so a bare `+x` means `a+x`, and `X` reads the mode as the clauses before it left it. `-v` prints `mode of 'x' changed from 0644 to 0755` |
| `ln` | `-s` / `--symbolic` | `-f` `-r` `-n` `-T` | modelled; exactly one target and one link name |
| `stat` | `-c FMT` / `--format=` / `--printf=`, `-L`; `%n %N %s %b %B %o %f %a %A %F %U %G %u %g %i %h %m %d %X %Y %Z %x %y %z %%` | `-f` `-t` `--cached`, any other conversion | modelled; see *Stat fidelity* below |
| `find` | `-name -iname -type f\|d -maxdepth` | every other predicate, refused by name | modelled |
| `grep` / `select-string` | `-i -v -n -c -l -L -F -E -G -q -s -h -H -w -x -r -R -e -o -A N -B N -C N`, long forms | `--include` `-P` `-m` | modelled; `0` matched, `1` did not. **A pattern is a basic regular expression unless `-E` says otherwise**, so `a\+` repeats and `a+` is a literal plus; the last of `-E`/`-G` wins. Context lines are prefixed with `-` where a matching line uses `:`, and `--` separates non-adjacent groups |
| `sed` | `-n -e -f -i[SUFFIX] -E -r -s --quiet --silent --expression --file --in-place --regexp-extended --separate`; addresses `N`, `$`, `/RE/`, `\cREc`, `N,M`, `first~step`, `addr,+N`, `addr,~N`, `0,/RE/`, the `I`/`M` regex modifiers and `!`; commands `{ }` `s` `y` `p` `P` `d` `D` `a` `i` `c` `r` `R` `w` `W` `n` `N` `h` `H` `g` `G` `x` `b` `t` `T` `:label` `q` `Q` `=` `l` `z` `F` `#`; `s` flags `g N p i/I m/M w FILE`, `&`, `\1`–`\9`, `\U \L \u \l \E` | `-z`, the `e` substitution flag, a backreference **inside a pattern** (`\(a\)\1` — the engine has no backtracking) | modelled; see *sed* below |
| `awk` / `gawk` / `mawk` / `nawk` | the POSIX language: patterns `/re/`, expressions, ranges, `BEGIN`/`END`; `$0`–`$NF` with assignment rebuilding `$0`; `NR NF FS OFS ORS RS FILENAME FNR SUBSEP RSTART RLENGTH CONVFMT OFMT ENVIRON`; `-F -v -f` (repeatable), `--field-separator --assign --file --source`; `if/else while for for-in do-while break continue next nextfile exit return delete`; arrays incl. multidimensional; user functions with local parameters and array parameters by reference; `length substr index split sub gsub match sprintf toupper tolower sin cos atan2 exp log sqrt int rand srand system close fflush`; `print`/`printf` with `> >> \| "cmd"`; every `getline` form | `-W`, gawk extensions (`gensub`, `asort`, `PROCINFO`, `RT`, `BEGINFILE`), `\|&` co-processes | modelled; see *awk* below |
| `xargs` | `-0 -d DELIM -n N -I REPL -r -t -P N --`, `--null --delimiter --max-args --replace --no-run-if-empty --verbose --max-procs`; quoting (`'…'`, `"…"`, `\`) | `-L` `-a` `-s` `-E` `-p` | modelled; **`-P` is accepted and commands still run one after another**, because a replay must be identical. Status `123` if any command failed, `124` for a command that exited 255, `126`/`127` passed through |
| `tr` | `SET1 [SET2]`, ranges `a-z`, `[:alpha:]` and the other classes, `[c*n]`, `[c*]`, escapes; `-d -s -c -t`, `--delete --squeeze-repeats --complement --truncate-set1` | operands beyond two | modelled, stdin only |
| `cut` | `-b -c -f -d -s --complement --output-delimiter`, ranges `N`, `N-`, `-M`, `N-M` and lists, on file operands as well as stdin | `-z`, `--characters` on multibyte boundaries other than `char` | modelled; a line with no delimiter passes through unless `-s` |
| `head` / `tail` | `-n N`, `-n +N`, `-n -N`, `-N` (bare count), `-c N`, `-q -v`, `--lines --bytes --quiet --silent --verbose`; several file operands with `==> name <==` headers | `-z`; **`tail -f` refused by name** | modelled; `head -n -N` drops the last N lines, `tail -n +N` starts at line N |
| `wc` | `-l -w -c -m -L`, `--lines --words --bytes --chars --max-line-length`, several operands with a `total` row | `-z` | modelled; the file name is printed beside the counts whenever an operand names one, as in coreutils |
| `sort` | `-n -g -h -r -u -f -b -M -V -c -s -d -i -k KEYDEF -t SEP -o FILE`, long forms; `KEYDEF` is `F[.C][opts][,F[.C][opts]]` with per-key `n g h M V f b d i r` | `-z`, `-m`, `-S`, `--parallel`, locale collation (the order is C/byte order) | modelled; without `-t` a field carries the blanks before it, which is what makes `-k2` and `-k2b` differ. `-c` reports `sort: FILE:N: disorder: LINE` and exits `1` |
| `uniq` | `-c -d -D -u -i -f N -s N -w N`, long forms | `-z`, `--group`, `--all-repeated=METHOD` | modelled over adjacent lines only, as coreutils does |
| `paste` | `-s`, `-d LIST` | `-z` | modelled; the delimiter list cycles |
| `join` | `-1 -2 -j -t -a N -v N -o LIST -e TEXT -i --check-order --nocheck-order` | `--header`, `-z` | modelled; `--nocheck-order` names the default, `--check-order` really checks and fails with status `1` |
| `comm` | `-1 -2 -3`, `--output-delimiter` | `--total`, `-z` | modelled; both inputs are assumed sorted, exactly as coreutils assumes |
| `diff` | `-u`/`-U N`, `-c`, `-q`, `-r`, `-s`, `-i`, `-w`, `-b`, `-B`, `-N`, long forms | `-y`, `--label`, `-D`, binary comparison | modelled with Myers' algorithm; `0` identical, `1` differ, `2` an error such as a missing operand |
| `tee` | `-a` / `--append`; `-i` accepted and inert **because this world delivers no signals** | `-p`, `--output-error` | modelled; `/dev/null` is discarded |
| `nl` | `-b a\|t\|n\|pRE`, `-n ln\|rn\|rz`, `-w N`, `-s STR`, `-v N`, long forms | `-p`, page sections (`\:\:\:`), `-d`, `-l`, `-f`, `-h` | modelled |
| `rev` | — | all flags | modelled |
| `fold` | `-w N`, `-N`, `-s`, `-b`, long forms | — | modelled |
| `expand` / `unexpand` | `-t LIST` / `--tabs`, `expand -i`, `unexpand -a` | `-t` with a multibyte tab character | modelled; each command refuses the other's flag |
| `shuf` | `-n N`, `-e`, `-i LO-HI`, `-r`, long forms | `-z`, `--random-source` | modelled; **seeded from the tick and the machine id**, so a replay prints the same permutation |
| `seq` | `[FIRST [INCR]] LAST`, `-s SEP`, `-w`, `-f FORMAT`, long forms | more than 1 000 000 values, refused | modelled |
| `yes` | `[STRING…]` | short flags are operands, as in coreutils; a long flag is refused | modelled with a published bound: **10 000 lines, then it stops**, because this world's pipelines are not lazy and a truly endless `yes` could never return. Use `seq`/`head` for an exact count |
| `basename` | `NAME [SUFFIX]`, `-a`, `-s SUFFIX`, long forms | `-z` | modelled |
| `dirname` | `NAME…` | `-z` | modelled |
| `realpath` / `readlink` | `-f -e -m -s -q`, long forms | `-z`, `--relative-to`, `--relative-base`, `readlink -n` | modelled over the VFS, following symlinks with a 16-hop bound. Bare `readlink` prints the link target and fails on a non-link |
| `split` | `-l N`, `-b SIZE` (with `b K M G`), `-a N`, `-d`, long forms | `-n CHUNKS`, `-C`, `--filter`, `--additional-suffix` | modelled; the default prefix is `x` and the default is 1000 lines |
| `strings` | `-n N`; `-a` accepted and inert **because every file here is scanned whole** | `-t`, `-e`, `-f` | modelled over the stored bytes |
| `base64` | `-d`, `-i`, `-w N`, long forms | `--base64url` | modelled; wraps at 76 columns, `-w0` never wraps |
| `md5sum` / `sha1sum` / `sha256sum` | `-c`; `-b` and `-t` accepted and inert **because the VFS has no text/binary distinction** | `--tag`, `--quiet`, `--status`, `--ignore-missing` | modelled; the digests are the real ones, computed from the stored bytes |
| `cmp` | `-s` / `--silent` / `--quiet`, `-l` / `--verbose` | `-i`, `-n`, `--bytes` | modelled; the `differ:` line goes to **stdout**, as in coreutils, and the status is `1` |
| `xxd` | default, `-p`, `-c N`, `-l N`, `-s N`, `-g N`, `-u`, `-r` (with and without `-p`), long forms | `-i`, `-b`, `-e`, `-s` with `+`/`-` | modelled |
| `od` | `-c -b -x -d -o`, `-A d\|o\|x\|n`, `-t c\|a\|x1\|o1\|d1\|o2`, `-N N`, `-j N`, `-v` | every other `-t` format, refused by name | modelled; repeated lines collapse to `*` unless `-v` |
| `hexdump` | default (two-byte octal), `-C -c -b -x -d -o`, `-n N`, `-s N`, `-v` | `-e` format strings | modelled |
| `file` | `-b`, `-i` / `--mime`, `-L`; `-h` accepted and inert **because not dereferencing is the default** | `-z`, `-f`, `--magic-file` | modelled; see *file* below |
| `du` | `-s -a -h -k -b -m -c -d N`, `--max-depth=`, `--summarize --all --human-readable --bytes --total` | `--exclude`, `-x`, `-L` | modelled over the VFS; block accounting assumes a 4 KiB allocation unit |
| `df` | `-h -k -T`, `--human-readable --print-type` | `-i` `-a` `-B` | **mixed**: capacity and device name are fixed, usage is summed from the VFS; one filesystem mounted at `/` |
| `which` | `-a`, `--all` | `-s` | modelled over `PATH`; see *which and builtins* below |
| `nproc` | `--all` | `--ignore` | **fixed** (`hardware.cpus`, default 4) |
| `uptime` | `-p -s`, `--pretty --since` | `-h` `-V` | modelled from the tick and `hardware.boot_tick`; the load average and user count are **fixed** at `0.00` and `1` |
| `clear` / `cls` | — | all arguments, refused by name | modelled as a screen action: no output, `CommandResult::clear` is set |
| `ip` | `addr` \| `a` \| `address`, `link` \| `l`, `route` \| `r`, optional `show`/`list` | `-4 -6 -o -brief` (the printout is a fixed block and cannot be filtered), every other object (`netns`, `tuntap`, `rule`, …) and every mutating action | **fixed** (`hardware.ipv4`, `prefix`, `mac`, `gateway`, `interface`) |
| `sudo` | `-u USER`, `--`, `-v -k -K` (succeed and do nothing); `-n -E` accepted and inert | password prompts, a sudoers policy, `-i`/`-s` login shells | modelled thinly: it swaps only the identity access checks use (default `root`, which bypasses VFS permissions). `HOME` and `USER` are left alone |
| `test` / `[` | `-e -f -d -s -r -w -x -L -h -n -z`, `= == != -eq -ne -lt -gt -le -ge -nt -ot` | `-a`/`-o`, `-p` `-S` `-g` `-u` `-k` | modelled against the VFS and its permissions |
| `[[ … ]]` | everything `test` takes, plus `&&` `\|\|` `!` and `( … )` inside the brackets, `==`/`!=` glob matching (an unquoted right side is a pattern, a quoted one a literal), `=~` regex, `<` `>` string order | `&&`-chaining onto other commands inside the brackets, `-v`, `-o` | modelled; a missing `]]` is a syntax error |
| `read` | `-r` (accepted; this shell never unescapes a read line), any number of names, the last taking the remainder; no name sets `REPLY` | `-p` `-t` `-n` `-d` `-s` `-u` `-a` and every long option, refused by name | modelled; fields split on whitespace. Status `1` at end of input, and also when the last line had no terminating newline — the same end-of-file report bash gives |
| `exit` | `[N]` | — | modelled; ends this shell (or this `sh -c`, script or subshell) with `N`, or with the last status. It does not end the caller's shell |
| `shift` | `[N]` | — | modelled; status `1` when `N` exceeds `$#`, which changes nothing |
| `local` | `NAME[=VALUE]…` | `-r` `-i` `-a` | modelled; shadows the name until the enclosing function returns. Outside a function it is refused with status `2` |
| `source` / `.` | `FILE [ARG…]` | — | modelled; runs the file in this shell, so its variables, working directory and functions persist. `return` ends it, `exit` ends the whole shell, and it counts against the nesting limit |
| `getopts` | `OPTSTRING NAME [ARG…]`, clusters (`-ab`), glued and separate option arguments, a leading `:` for silent mode | `--long` options | modelled; `OPTIND` and `OPTARG` are ordinary shell variables, so resetting `OPTIND=1` restarts the scan |
| `ps` / `Get-Process` | `-e` / `-A`, `-f`, `-u USER`, `-p PID`, `--json` | `aux` and every other BSD operand, `-o` `-l` `--forest` | modelled; **column output by default**. `TTY` is `?` and `TIME` is `00:00:00` for every process because no terminal and no CPU accounting are simulated; a zombie prints `<defunct>`. With no selector `ps` lists the current user's processes. `--json` dumps the whole table as JSON — the pre-column behaviour, kept for machine consumers |
| `kill` | `-SIGNAL` / `-N` for `TERM KILL INT STOP TSTP CONT HUP QUIT USR1 USR2 0` | `-l`, `-s NAME`, any other signal name — refused by name rather than delivered as a no-op | modelled against the process table and its signal dispositions |
| `sleep` | fractional seconds, trailing `&` | — | modelled against simulated time; never blocks the host |
| `systemctl` / `service` | `start stop restart status` | every flag (`--user`, `--now`, `--no-pager` … all imply machinery this world lacks), `enable` `disable` `daemon-reload` | modelled against the process table and the service adapter |
| `apt` / `apt-get` / `brew` / `winget` / `pip` / `npm` | `install`, `remove`/`uninstall`, `list` | `update` `upgrade` `search` | modelled against the package manager, offline |
| `curl` / `wget` | `-X` / `--request`, `-d` / `--data` / `--data-raw`, `-H` / `--header`, `-o` / `--output`, `-f` / `--fail`; `-s` / `--silent` and `-S` / `--show-error` accepted and inert **because there is no progress meter and no TTY** | `-L` `-I` `-u` `-k` `-A`, and every other flag, refused by name | modelled against the network adapter |
| `git` | `init`, `clone`, `add [-A] PATH…`, `status`, `commit -m MSG` (`-a`/`-am`), `log`, `diff [--staged\|--cached]`, `reset [--soft\|--mixed\|--hard] [REV] [--] [PATH…]`, `restore [--staged] [--worktree] [--source=REV] PATH…`, `checkout`/`switch [-b\|-c] BRANCH`, `checkout -- PATH…`, `branch [NAME]`, `remote [add NAME URL]`, `fetch`, `pull`, `push`, `config KEY [VALUE]`, `-C DIR` | every other subcommand and every global option, refused by name with status `2` | modelled, content-addressed (`crates/computer/src/git.rs`): objects, refs and the index are the repository's own state under `.git/state.json`, and the worktree is the machine's files. See *git* below |
| `sqlite3` | `[OPTIONS] [FILE [SQL…]]`; SQL and dot-commands on stdin (pipe, heredoc, `<`); `-header -noheader -csv -column -list -line -json -box -table -markdown -tabs -quote -html -ascii -separator SEP -newline SEP -nullvalue TEXT -cmd CMD -init FILE -bail -echo -version -help`; `-batch -readonly -safe` accepted and inert | every other option, refused by name with status `2`; an interactive prompt | modelled: the `cw-sql` engine over the VFS, reading and writing real SQLite 3 files; see *sqlite3* below |
| `sh` / `bash` | `-c SCRIPT [NAME [ARG…]]`, script path plus arguments | `-e` `-x` | modelled; a nested run of the same shell, with its own budget and its own function table |
| `break` / `continue` / `return` | `[N]` | — | modelled as shell signals; see *Grammar* |
| `python3` / `python` | `FILE [ARG…]`, `-c CODE`, `-m MODULE`, `-` or no operand (program on stdin), `-V` / `--version`, `-h`; `-B -E -I -O -q -s -S -u -v -d -b -i -W ARG -X OPT` accepted and inert | `pip` inside the interpreter, C extensions, threads, sockets, subprocesses | modelled by an in-process CPython 3.12 interpreter; see *Language runtimes* below |
| `node` / `nodejs` | `FILE [ARG…]` (`.js`, `.cjs`, `.mjs`), `-e` / `--eval`, `-p` / `--print`, `-c` / `--check`, `-r` / `--require`, `--input-type=module`, `--stack-trace-limit=N`, `-` or no operand (program on stdin), `-v` / `--version`, `-h`; V8 and diagnostic flags (`--no-warnings`, `--max-old-space-size=…`, `--experimental-*`, …) accepted and inert | the REPL (`-i` runs the program without one), `--inspect`, `--watch`, `--test`, native addons, `worker_threads`, networking modules, `child_process` (fails with `ENOSYS`) | modelled by an in-process ES2023 interpreter with Node 24.21 semantics; see *Language runtimes* below |
| PowerShell aliases | `Write-Output Get-Location Set-Location Get-ChildItem Get-Content Set-Content Add-Content Copy-Item Move-Item Remove-Item Select-String Get-Process Stop-Process Invoke-WebRequest Test-Path` | the rest of PowerShell | modelled; only available when the computer's dialect is `powershell` |
| anything else | — | — | status `127`, `command not found` |

## awk

`awk` (`crates/computer/src/awk.rs`) is the POSIX language, not a field-printing
shortcut: a lexer, a recursive-descent parser and an interpreter with the whole value
model. `gawk`, `mawk` and `nawk` are the same command.

**Values.** A scalar is uninitialised, a number, a string, or a *string from input*.
The last is the rule that makes real scripts work: a field or a `getline` result that
reads entirely as a number compares numerically, so `$1 == 10` is true for a line
containing `10.0`, while `"10" == 10` compares the string constant as a string. An
uninitialised value equals both `0` and `""`. Numbers print as integers when they are
integral and through `CONVFMT` (`OFMT` for `print`) when they are not.

**Records and fields.** `RS` is a single character, a multi-character regular
expression, or `""` for paragraph mode (a blank line separates records and a newline
always separates fields). `FS` is a single character taken literally, a regular
expression when longer, `" "` for the default blank-run split, and `""` to split into
characters; `-Ft` means a tab, as in every awk. Assigning `$n` past `NF` pads the
record, assigning `NF` truncates it, and either rebuilds `$0` with `OFS`.

**Determinism.** `for (k in a)` walks the subscripts in **sorted order**. POSIX leaves
the order unspecified; this world fixes it so a replay is identical. `rand()` is a
48-bit LCG seeded from the world; `srand()` with no argument seeds from the simulated
tick, not a host clock, and returns the previous seed.

**Streams.** `print > "file"` and `print >> "file"` buffer and write through the VFS.
`print | "cmd"` buffers its text and runs `cmd` when the pipe is **closed** — by
`close("cmd")`, `fflush()`, `system()`, or the end of the program — and the command's
output is spliced into awk's own at that moment. There is no second process to
schedule, so this is the honest ordering; it makes `print | "sort"` behave exactly as
expected. `"cmd" | getline` runs the command once and reads its output as records.
`getline < "file"` returns `1`, `0` at end of file, and `-1` when the file cannot be
opened — it never aborts the program.

**Function parameters.** Parameters beyond the arguments are locals. Whether a
parameter is a scalar or an array is decided from how the function body uses it
(subscripted, walked with `for … in`, `delete`d, filled by `split`, or passed on to
another function's array parameter), computed once as a fixpoint over the whole
program; an array parameter is shared with the caller by reference.

**Bounds.** A program is stopped with status `2` after 2 000 000 evaluation steps or
256 nested function calls, so a runaway `while(1)` ends rather than hanging the world.

`awk --version` and `sed --version` print `… (computerworld) POSIX profile`, so a
script that probes for a GNU-only feature by version string gets an honest answer
rather than a number it can compare against.

## sed

`sed` (`crates/computer/src/sed.rs`) runs a real cycle: a pattern space, a hold space,
an append queue, branch labels and a program counter. `-i` and `-s` process each file
separately; otherwise every operand is one stream, so `$` is the last line of the last
file and line numbers run on.

Basic and extended regular expressions really differ. In a BRE, `\(…\)` groups,
`\{n,m\}` repeats, `\|` alternates and `\+`/`\?` are GNU's extensions, while the bare
characters are literals; `*` is a literal at the start of an expression and `^`/`$`
anchor only at the edges. `-E` (or `-r`) swaps the two. `\<` and `\>` both become a
word boundary, because the engine has no lookaround. A backreference **inside** a
pattern is refused by name — the engine cannot backtrack — while `\1`–`\9` in a
replacement work, as do `&`, `\&`, and GNU's `\U \L \u \l \E` case operators.

`a`, `i` and `c` take both the POSIX `a\` + text form and GNU's one-line `a text`.
`c` on a range prints its text once, at the end of the range. `w` and `s///w` write
through the VFS (with `/dev/stdout` writing to standard output), `r` and `R` read from
it. `q`'s code becomes the exit status and everything already printed still reaches the
caller; `Q` quits without the final auto-print.

## file

`file` reads the stored bytes and reports only what this world can actually produce, so
it never guesses:

| Magic | Reported as |
| --- | --- |
| empty file | `empty` |
| `\x89PNG\r\n\x1a\n` | `PNG image data, W x H, D-bit/color KIND, non-interlaced`, with `, APNG` appended when an `acTL` chunk is present |
| `\xff\xd8\xff` | `JPEG image data, JFIF standard` |
| `GIF87a` / `GIF89a` | `GIF image data` |
| `RIFF….WAVE` | `RIFF (little-endian) data, WAVE audio` |
| `SQLite format 3\0` | `SQLite 3.x database` |
| `%PDF-` | `PDF document, version N.N` |
| `PK\x03\x04` | `Zip archive data`, or `Microsoft Excel 2007+` / `Word` / `PowerPoint` / `OpenDocument …` when the member names say so |
| `#!` | `NAME script, ASCII text executable` |
| `\x7fELF` | `ELF binary (this world runs no native executables)` — nothing here writes one |
| valid UTF-8, no control characters | `ASCII text`, `Unicode text, UTF-8 text`, `JSON text data` or `CSV text`, with `, with no line terminators` when the last line is unterminated |
| anything else | `data` |

A directory is `directory` and a symlink is `symbolic link to TARGET` unless `-L`
follows it. `-i` maps the same table onto a MIME type.

## git

`git` is a synthetic, content-addressed Git: commits hold whole file trees, the index is
a real staging area and the worktree is the machine's own files, so `status`, `diff` and
a commit all say what the disk says. A revision is `HEAD`, `HEAD~<n>` (or `HEAD^…`), a
branch name, or a commit hash (a unique prefix of at least four characters is enough).

| Command | Effect |
|---|---|
| `git diff` / `git diff --staged` (`--cached`) | The worktree against the index, or the index against `HEAD` |
| `git reset [REV]` | `--mixed` (the default) moves the branch to `REV` and resets the index to it, leaving the worktree alone and listing what is now unstaged; `--soft` moves the branch only; `--hard` also throws away the working copies |
| `git reset [REV] [--] PATH…` | Copies those paths from `REV` (default `HEAD`) into the index, leaving the worktree alone: this is what unstages a file. A `--soft` or `--hard` reset with paths is refused, as git refuses it |
| `git restore PATH…` | The worktree comes back from the index (Discard Changes) |
| `git restore --staged PATH…` | The index comes back from `HEAD` (Unstage); `--staged --worktree` does both, and `--source=REV` takes the content from another commit |
| `git checkout -- PATH…` | The older spelling of `git restore PATH…` |

A folder (or `.`) as a path stands for every file under it. Visual Studio Code's Source
Control view runs exactly these commands: its `+` is `git add`, its `−` is `git restore
--staged`, Unstage All Changes is `git reset`, and Discard Changes is `git restore` (an
untracked file is moved to the trash instead).

## sqlite3

`sqlite3` runs the pure `cw-sql` engine (`crates/sql`), which follows SQLite 3.45.1's
dialect, messages and shell output. `FILE` is resolved against the working directory and
read and written through the same permission checks as `cat` and a redirect; `:memory:`
or no file is a scratch database. Every argument after `FILE` is run in order (SQL or a
dot-command) and the first error ends the run; with no SQL arguments the shell reads its
standard input instead, so `echo 'select 1;' | sqlite3 f.db`, a heredoc and `< script.sql`
all work. There is no interactive prompt: each terminal line is one invocation.

| Dot-command | Behaviour |
| --- | --- |
| `.tables ?PATTERN?` / `.indexes ?TABLE?` | names in columns, as the real shell lays them out |
| `.schema ?PATTERN?` | stored `CREATE` text, one statement per line |
| `.mode MODE ?TABLE?` | `list csv tabs column table box markdown json line insert quote html ascii`; `csv` ends rows with CRLF and `column` turns headers on |
| `.headers on\|off`, `.separator COL ?ROW?`, `.nullvalue TEXT`, `.width N…` | output settings; `.show` prints them |
| `.import ?--csv? ?--skip N? FILE TABLE` | a missing table is created from the header row with `TEXT` columns; short and long rows are filled or trimmed with the shell's warnings |
| `.dump ?TABLE?` / `.read FILE` | SQL text out and back in |
| `.open ?--new? FILE`, `.save FILE`, `.output ?FILE?`, `.once FILE` | switch databases, copy one, redirect output to a file |
| `.bail`, `.echo`, `.changes`, `.print`, `.databases`, `.help` | as in SQLite |
| `.quit` / `.exit ?CODE?` | stop; the exit status is `CODE`, or `1` if anything failed |

Errors use the real shell's wording and go to stderr: `Error: in prepare, …` and
`Error: stepping, … (19)` for arguments, `Parse error near line N: …` and
`Runtime error near line N: …` for scripts, with a caret under syntax errors. The status
is `1` when any statement failed. A database is written back only when its content
changed, as one whole-file VFS write (so no reader ever sees half a save); a file that
was only read is never created, and a transaction still open at exit is rolled back.
`'now'` and `CURRENT_TIMESTAMP` read the simulated clock, and `random()` is a seeded
stream, so every replay agrees.

The engine implements tables with `PRIMARY KEY`, `NOT NULL`, `UNIQUE`, `CHECK`,
`DEFAULT`, `COLLATE` and `REFERENCES` (enforced with `PRAGMA foreign_keys = ON`,
including `CASCADE`, `SET NULL` and `SET DEFAULT`), `AUTOINCREMENT`, `WITHOUT ROWID`
tables (stored, as SQLite stores them, as an index B-tree in primary key order),
B-tree indexes the planner uses for equality, `IN` and range lookups and to avoid
sorts (`EXPLAIN QUERY PLAN` shows the choice, including `USING COVERING INDEX` when
the index holds every column the query reads and `USING PRIMARY KEY` for a WITHOUT
ROWID table), views, triggers (`BEFORE`, `AFTER` and `INSTEAD OF` on `INSERT`,
`UPDATE [OF columns]` and `DELETE`, `FOR EACH ROW` with `WHEN`, `NEW` and `OLD`, and
`RAISE(IGNORE | ABORT | FAIL | ROLLBACK, message)`; the newest fires first,
`recursive_triggers` is off, foreign key actions fire the child's triggers, and a
view with `INSTEAD OF` triggers takes writes), `ALTER TABLE` (rename, add, rename and
drop column; renames rewrite triggers, indexes and the views that read the table,
quoting the new name as the statement did), joins (inner, left, right, full, cross,
`USING`, `NATURAL`), grouping, aggregates, compound selects, scalar, `IN` and `EXISTS`
subqueries, recursive CTEs, upsert, `RETURNING`, transactions and savepoints. Refused
by name rather than half-done: window functions, partial and expression indexes,
generated columns, `ATTACH`, virtual tables, JSON operators and bytecode `EXPLAIN`.
Foreign keys are checked immediately rather than deferred to the end of the
statement.

## Language runtimes

`python3` (crate `cw-pyvm`) and `node` (crate `cw-jsvm`) are interpreters written in
Rust that run inside the simulation. They are commands like any other: they resolve
through `PATH` and `which`, read stdin from a pipe or here-document, write to the pipe
or redirection that follows them, and set `$?`. A script with a `#!/usr/bin/env
python3`, `#!/usr/bin/python3`, `#!/usr/bin/env node` or `#!/usr/bin/node` line runs
under that interpreter when executed by path after `chmod +x`.

Both see exactly what the rest of the shell sees and nothing of the host:

* **Files** are the computer's VFS, with the current user's permissions, relative to the
  shell's working directory. A program's `os.chdir` / `process.chdir` moves only the
  program, never the shell.
* **Time** is the simulated clock. `time.time()`, `Date.now()` and `new Date()` start at
  the world tick; timers, `time.sleep`, `setTimeout` and `setInterval` advance a
  virtual clock and never block the host. Executing code takes virtual time as well
  (one millisecond per 100 000 `node` instructions), so a busy-wait on `Date.now()`
  ends. The timezone is UTC.
* **Randomness** (`random`, `secrets`, `Math.random`, `crypto.randomBytes`,
  `crypto.randomUUID`) is drawn from the world's seeded entropy, so a replay prints the
  same numbers. `random.seed(n)` streams match CPython exactly.
* **Resources** are bounded: a program that exceeds its instruction budget (50 million
  steps for `python3`, 200 million for `node`) stops with a `TimeoutError` and status
  `124`; the budget cannot be caught. Deep recursion is Python's `RecursionError`
  or Node's `RangeError: Maximum call stack size exceeded`, not a host crash.

Output reproduces the real tools, byte for byte where it is observable: `print`, `repr`
and tracebacks for Python; `console.log` / `util.inspect` formatting, uncaught-error
reports (source line, caret, stack with Node's internal frames, `Node.js v24.21.0`),
unhandled rejections and exit codes for Node. Conformance corpora of whole programs
with outputs recorded from CPython 3.12 and Node 24.21 live in
`crates/pyvm/tests/programs` and `crates/jsvm/tests/programs`.

`node` implements the language through ES2023 (classes with private members,
generators, async functions and async iterators, destructuring, spread, optional
chaining, BigInt, tagged templates, Proxy/Reflect, typed arrays, `DataView`, WeakRef,
labelled statements, getters and setters, ES modules with top-level `await` and dynamic
`import()`), CommonJS `require` with Node's resolution (`node_modules`, `index.js`,
`package.json` `main`, JSON files, `require.cache`), and a Node-shaped event loop
(`process.nextTick`, microtasks, timers, immediates, `process.on('exit')`,
`'uncaughtException'` and `'unhandledRejection'`). Built-in modules: `fs` (sync,
callback and promise APIs), `fs/promises`, `path`, `os`, `events`, `util`, `assert`
(`assert/strict`), `readline` (`readline/promises`), `url`, `querystring`,
`string_decoder`, `stream` (a subset), `buffer`, `crypto` (hashes, HMAC, random),
`timers`, `timers/promises`, `perf_hooks`, `process` and `child_process` (which refuses
with `ENOSYS`). Globals include `Buffer`, `URL`, `URLSearchParams`, `TextEncoder`,
`TextDecoder`, `AbortController`, `structuredClone`, `atob`/`btoa`, `queueMicrotask`
and a `crypto` object.

Known gaps shared by both: no network access, no subprocesses, no threads and no
native extensions. `node` does not implement `Intl` beyond `en-US` date and number
formatting, `Atomics`/`SharedArrayBuffer`, `http`/`net`/`dns`/`zlib`/`worker_threads`,
or the REPL. Strings that contain unpaired UTF-16 surrogates are carried as the
replacement character. Event-loop orderings that depend on real wall-clock jitter in
Node (for example `setTimeout(f, 0)` against `setImmediate(g)` from the main module)
are resolved one fixed way: the main module is taken to run for one millisecond.

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
* A regex backreference *inside* a pattern (`sed 's/\(a\)\1/x/'`, `grep '\(a\)\1'`).
  The engine is a finite automaton with no backtracking, so it is refused by name
  rather than quietly matching something else. Backreferences in a `sed` *replacement*
  (`\1`–`\9`) work.
* `awk`'s `|&` co-processes and gawk's extension library (`gensub`, `asort`,
  `PROCINFO`, `RT`, `BEGINFILE`/`ENDFILE`). A `print | "cmd"` pipe buffers its text and
  runs the command when the pipe closes, because this world has no second process to
  schedule; `close()` is therefore load-bearing where gawk would also accept a flush.
* `tail -f`: nothing in this world changes a file except a command in this same shell,
  so a follow would never wake. It is refused by name.
* Locale collation: `sort` and every comparison use C/byte order, and `[a-z]` in a
  bracket expression means the ASCII range.
* `ps` columns that would have to be invented: `%CPU`, `%MEM`, `VSZ`, `RSS`, `STAT`.
  `ps aux` is refused by name rather than filled with plausible numbers.
* Real process scheduling: only `sleep` occupies simulated time.
* A group database, an allocator, and per-file access/change times: `touch -a` and
  `touch -m` both move the single stored timestamp.
* Binary bytes cannot travel through a pipe or a redirect: the shell's streams are
  UTF-8 strings, so `printf '\211PNG'` writes the UTF-8 encoding of U+0089, not the
  byte `0x89`. Commands that *read* bytes (`file`, `xxd`, `od`, `hexdump`, `cmp`,
  `strings`, `md5sum` and friends) read them straight from the VFS and are exact.
