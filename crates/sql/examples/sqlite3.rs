//! The engine's `sqlite3` shell over the host filesystem, for comparing its behaviour
//! with the real `sqlite3`: `cargo run -p cw-sql --example sqlite3 -- file.db "SELECT 1"`.
//! The clock is pinned to the world's origin so output is reproducible.
use std::io::Read;

struct HostFiles;
impl cw_sql::cli::Host for HostFiles {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdin = String::new();
    if args.iter().filter(|a| !a.starts_with('-')).count() < 2 {
        let _ = std::io::stdin().read_to_string(&mut stdin);
    }
    let r = cw_sql::cli::run(&args, &stdin, &mut HostFiles, 1_789_635_600_000_000);
    print!("{}", r.stdout);
    eprint!("{}", r.stderr);
    std::process::exit(r.code);
}
