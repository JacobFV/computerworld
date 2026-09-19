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
| `git` | `init`, `clone`, `add [-A] PATH…`, `status`, `commit -m MSG` (`-a`/`-am`), `log`, `diff [--staged\|--cached]`, `reset [--soft\|--mixed\|--hard] [REV] [--] [PATH…]`, `restore [--staged] [--worktree] [--source=REV] PATH…`, `checkout`/`switch [-b\|-c] BRANCH`, `checkout -- PATH…`, `branch [NAME]`, `remote [add NAME URL]`, `fetch`, `pull`, `push`, `config KEY [VALUE]`, `-C DIR` | every other subcommand and flag, refused by name | modelled, content-addressed (`crates/computer/src/git.rs`): objects, refs and the index are the repository's own state under `.git/state.json`, and the worktree is the machine's files. See *git* below |
| `sqlite3` | `[OPTIONS] [FILE [SQL…]]`; SQL and dot-commands on stdin (pipe, heredoc, `<`); `-header -noheader -csv -column -list -line -json -box -table -markdown -tabs -quote -html -ascii -separator SEP -newline SEP -nullvalue TEXT -cmd CMD -init FILE -bail -echo -version -help`; `-batch -readonly -safe` accepted and inert | every other option, refused by name with status `2`; an interactive prompt | modelled: the `cw-sql` engine over the VFS, reading and writing real SQLite 3 files; see *sqlite3* below |
| `sh` / `bash` | `-c SCRIPT [NAME [ARG…]]`, script path plus arguments | `-e` `-x` | modelled; a nested run of the same shell, with its own budget and its own function table |
| `break` / `continue` / `return` | `[N]` | — | modelled as shell signals; see *Grammar* |
| `python3` / `python` | `FILE [ARG…]`, `-c CODE`, `-m MODULE`, `-` or no operand (program on stdin), `-V` / `--version`, `-h`; `-B -E -I -O -q -s -S -u -v -d -b -i -W ARG -X OPT` accepted and inert | `pip` inside the interpreter, C extensions, threads, sockets, subprocesses | modelled by an in-process CPython 3.12 interpreter; see *Language runtimes* below |
| `node` / `nodejs` | `FILE [ARG…]` (`.js`, `.cjs`, `.mjs`), `-e` / `--eval`, `-p` / `--print`, `-c` / `--check`, `-r` / `--require`, `--input-type=module`, `--stack-trace-limit=N`, `-` or no operand (program on stdin), `-v` / `--version`, `-h`; V8 and diagnostic flags (`--no-warnings`, `--max-old-space-size=…`, `--experimental-*`, …) accepted and inert | the REPL (`-i` runs the program without one), `--inspect`, `--watch`, `--test`, native addons, `worker_threads`, networking modules, `child_process` (fails with `ENOSYS`) | modelled by an in-process ES2023 interpreter with Node 24.21 semantics; see *Language runtimes* below |
| PowerShell aliases | `Write-Output Get-Location Set-Location Get-ChildItem Get-Content Set-Content Add-Content Copy-Item Move-Item Remove-Item Select-String Get-Process Stop-Process Invoke-WebRequest Test-Path` | the rest of PowerShell | modelled; only available when the computer's dialect is `powershell` |
| anything else | — | — | status `127`, `command not found` |

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
* `sed`'s hold space, `b`/`t` branching, `N`/`n`, `w`/`r`, and more than one `-e`.
* `ps` columns that would have to be invented: `%CPU`, `%MEM`, `VSZ`, `RSS`, `STAT`.
  `ps aux` is refused by name rather than filled with plausible numbers.
* Real process scheduling: only `sleep` occupies simulated time.
* A group database, an allocator, and per-file access/change times: `touch -a` and
  `touch -m` both move the single stored timestamp.
