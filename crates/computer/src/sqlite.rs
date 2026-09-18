//! `sqlite3`: the cw-sql shell over this computer's filesystem. Database files are the
//! real SQLite 3 format, read and written through the same permission checks `cat` and
//! a redirect use, and `'now'` is the machine's simulated clock.
use crate::{CommandResult, Computer};

struct MachineFiles<'a> {
    computer: &'a mut Computer,
    tick: u64,
}
impl cw_sql::cli::Host for MachineFiles<'_> {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.computer.resolve(path);
        if !self.computer.vfs.exists(&path) {
            return Ok(None);
        }
        self.computer
            .vfs
            .read_as(&path, &self.computer.user)
            .map(Some)
            .map_err(|e| e.to_string())
    }
    /// One VFS write replaces the whole file, so a database is never seen half-saved.
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.computer.resolve(path);
        let user = self.computer.user.clone();
        self.computer
            .vfs
            .write_as(&path, bytes, &user, self.tick)
            .map_err(|e| e.to_string())
    }
}

/// Run `sqlite3 ARGS...` with `input` as its standard input.
pub fn execute(c: &mut Computer, args: &[String], input: &str, tick: u64) -> CommandResult {
    let now_us = (crate::shell::EPOCH_UNIX_SECONDS as i64)
        .saturating_mul(1_000_000)
        .saturating_add(tick as i64);
    let result = cw_sql::cli::run(
        &args[1..],
        input,
        &mut MachineFiles { computer: c, tick },
        now_us,
    );
    CommandResult {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.code,
        ..CommandResult::default()
    }
}
/// Whether this invocation reads SQL from standard input: it does unless SQL was given
/// as arguments after the database name.
pub fn reads_stdin(args: &[String]) -> bool {
    let mut positional = 0;
    let mut skip = false;
    for a in &args[1..] {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with('-') && positional == 0 {
            skip = matches!(
                a.trim_start_matches('-'),
                "separator" | "newline" | "nullvalue" | "cmd" | "init"
            );
            continue;
        }
        positional += 1;
    }
    positional < 2
}
