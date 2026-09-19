//! The `sqlite3` command-line shell: argument handling, dot-commands and output modes,
//! following SQLite 3.45's shell. File access goes through [`Host`], so the machine
//! that runs it decides what a path means.
use crate::value::{format_real, Value};
use crate::{Database, Output, SqlError};

/// The filesystem the shell sees.
pub trait Host {
    /// `Ok(None)` when nothing is at `path`.
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, String>;
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), String>;
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    List,
    Csv,
    Tabs,
    Column,
    Table,
    Box,
    Markdown,
    Json,
    Line,
    Insert,
    Quote,
    Html,
    Ascii,
}
impl Mode {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "list" => Self::List,
            "csv" => Self::Csv,
            "tabs" => Self::Tabs,
            "column" => Self::Column,
            "table" => Self::Table,
            "box" => Self::Box,
            "markdown" => Self::Markdown,
            "json" => Self::Json,
            "line" => Self::Line,
            "insert" => Self::Insert,
            "quote" => Self::Quote,
            "html" => Self::Html,
            "ascii" => Self::Ascii,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Csv => "csv",
            Self::Tabs => "tabs",
            Self::Column => "column",
            Self::Table => "table",
            Self::Box => "box",
            Self::Markdown => "markdown",
            Self::Json => "json",
            Self::Line => "line",
            Self::Insert => "insert",
            Self::Quote => "quote",
            Self::Html => "html",
            Self::Ascii => "ascii",
        }
    }
}

struct Shell<'h> {
    host: &'h mut dyn Host,
    db: Database,
    /// What the file held when opened, to tell whether anything needs writing.
    opened: Database,
    path: Option<String>,
    mode: Mode,
    insert_table: String,
    headers: bool,
    headers_set: bool,
    colsep: String,
    rowsep: String,
    nullvalue: String,
    widths: Vec<usize>,
    bail: bool,
    echo: bool,
    changes: bool,
    out: String,
    err: String,
    /// `.output FILE` / `.once FILE` capture, flushed to the file.
    redirect: Option<(String, bool, String)>,
    errors: usize,
    quit: Option<i32>,
    now_us: i64,
    open_error: Option<SqlError>,
}

const HELP: &str = "\
.bail on|off             Stop after hitting an error.  Default OFF
.changes on|off          Show number of rows changed by SQL
.databases               List names and files of attached databases
.dump ?TABLE?            Render database content as SQL
.echo on|off             Turn command echo on or off
.exit ?CODE?             Exit this program with return-code CODE
.headers on|off          Turn display of headers on or off
.help                    Show this message
.import FILE TABLE       Import data from FILE into TABLE
.indexes ?TABLE?         Show names of indexes
.mode MODE ?TABLE?       Set output mode
.nullvalue STRING        Use STRING in place of NULL values
.once FILE               Output for the next SQL command only to FILE
.open ?--new? FILE       Close existing database and reopen FILE
.output ?FILE?           Send output to FILE or stdout if FILE is omitted
.print STRING...         Print literal STRING
.quit                    Exit this program
.read FILE               Read input from FILE
.save FILE               Write the database to FILE
.schema ?PATTERN?        Show the CREATE statements matching PATTERN
.separator COL ?ROW?     Change the column and row separators
.show                    Show the current values for various settings
.tables ?TABLE?          List names of tables matching LIKE pattern TABLE
.width NUM1 NUM2 ...     Set minimum column widths for columnar output
";

/// Run `sqlite3` with `args` (not including the program name) and `stdin`.
pub fn run(args: &[String], stdin: &str, host: &mut dyn Host, now_us: i64) -> CliResult {
    let mut shell = Shell {
        host,
        db: Database::new(),
        opened: Database::new(),
        path: None,
        mode: Mode::List,
        insert_table: "table".into(),
        headers: false,
        headers_set: false,
        colsep: "|".into(),
        rowsep: "\n".into(),
        nullvalue: String::new(),
        widths: vec![],
        bail: false,
        echo: false,
        changes: false,
        out: String::new(),
        err: String::new(),
        redirect: None,
        errors: 0,
        quit: None,
        now_us,
        open_error: None,
    };
    shell.db.set_now(now_us);
    shell.opened = shell.db.clone();
    let mut file = None;
    let mut commands: Vec<String> = Vec::new();
    let mut pre: Vec<String> = Vec::new();
    let mut init: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let opt = a
            .strip_prefix("--")
            .map(|s| format!("-{s}"))
            .unwrap_or_else(|| a.clone());
        if a.starts_with('-') && a.len() > 1 && commands.is_empty() {
            let mut value = || {
                i += 1;
                args.get(i).cloned()
            };
            match opt.as_str() {
                "-header" => {
                    shell.headers = true;
                    shell.headers_set = true;
                }
                "-noheader" => {
                    shell.headers = false;
                    shell.headers_set = true;
                }
                "-bail" => shell.bail = true,
                "-batch" | "-readonly" | "-safe" | "-nofollow" => {}
                "-echo" => shell.echo = true,
                "-version" => {
                    return CliResult {
                        stdout: format!("{} computerworld cw-sql\n", crate::SQLITE_VERSION),
                        ..CliResult::default()
                    }
                }
                "-help" => {
                    return CliResult {
                        stderr: String::from(
                            "Usage: sqlite3 [OPTIONS] FILENAME [SQL]\nFILENAME is the name of an SQLite database. A new database is created\nif the file does not previously exist. Defaults to :memory:.\nOPTIONS include:\n   -bail -batch -box -cmd COMMAND -column -csv -echo -header -html -init FILENAME\n   -json -line -list -markdown -newline SEP -noheader -nullvalue TEXT -quote\n   -readonly -separator SEP -table -tabs -version\n"
                        ),
                        ..CliResult::default()
                    }
                }
                "-separator" | "-newline" | "-nullvalue" | "-cmd" | "-init" => {
                    let Some(v) = value() else {
                        return CliResult {
                            stderr: format!("sqlite3: Error: missing argument to {a}\n"),
                            code: 1,
                            ..CliResult::default()
                        };
                    };
                    match opt.as_str() {
                        "-separator" => shell.colsep = unescape(&v),
                        "-newline" => shell.rowsep = unescape(&v),
                        "-nullvalue" => shell.nullvalue = unescape(&v),
                        "-cmd" => pre.push(v),
                        _ => init = Some(v),
                    }
                }
                m => {
                    let name = &m[1..];
                    match Mode::parse(name) {
                        Some(mode) => shell.set_mode(mode),
                        None => {
                            return CliResult {
                                stderr: format!(
                                    "sqlite3: Error: unknown option: {a}\nUse -help for a list of options.\n"
                                ),
                                code: 2,
                                ..CliResult::default()
                            }
                        }
                    }
                }
            }
        } else if file.is_none() && commands.is_empty() {
            file = Some(a.clone());
        } else {
            commands.push(a.clone());
        }
        i += 1;
    }
    if let Some(f) = file.filter(|f| f != ":memory:" && !f.is_empty()) {
        shell.open(&f, false);
    }
    if let Some(path) = init {
        match shell.host.read(&path) {
            Ok(Some(bytes)) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                shell.script(&text);
            }
            _ => shell.error(format!("cannot open \"{path}\"")),
        }
    }
    for c in pre {
        if shell.quit.is_some() {
            break;
        }
        shell.argument(&c);
    }
    if commands.is_empty() {
        shell.script(stdin);
    } else {
        for c in &commands {
            if shell.quit.is_some() {
                break;
            }
            if !shell.argument(c) {
                break;
            }
        }
    }
    shell.finish()
}
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn quote_ident(name: &str) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !crate::parser::is_keyword(name);
    if plain {
        name.into()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}
fn width(s: &str) -> usize {
    s.chars().count()
}

impl Shell<'_> {
    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        match mode {
            Mode::Csv => {
                self.colsep = ",".into();
                self.rowsep = "\r\n".into();
            }
            Mode::Tabs => {
                self.colsep = "\t".into();
                self.rowsep = "\n".into();
            }
            Mode::Ascii => {
                self.colsep = "\u{1f}".into();
                self.rowsep = "\u{1e}".into();
            }
            Mode::List => {
                if self.colsep == "," || self.colsep == "\t" || self.colsep == "\u{1f}" {
                    self.colsep = "|".into();
                }
                self.rowsep = "\n".into();
            }
            Mode::Column if !self.headers_set => self.headers = true,
            _ => {}
        }
    }
    fn print(&mut self, text: &str) {
        match &mut self.redirect {
            Some((_, _, buf)) => buf.push_str(text),
            None => self.out.push_str(text),
        }
    }
    fn error(&mut self, text: impl AsRef<str>) {
        self.errors += 1;
        self.err.push_str(text.as_ref());
        if !text.as_ref().ends_with('\n') {
            self.err.push('\n');
        }
    }
    fn open(&mut self, path: &str, new: bool) {
        self.flush_db();
        self.path = Some(path.to_owned());
        self.open_error = None;
        let bytes = if new {
            None
        } else {
            match self.host.read(path) {
                Ok(b) => b,
                Err(e) => {
                    self.open_error = Some(
                        SqlError::new(format!("unable to open database file: {e}")).with_code(14),
                    );
                    None
                }
            }
        };
        match Database::open(bytes.as_deref().unwrap_or(&[])) {
            Ok(db) => {
                self.db = db;
                self.db.set_now(self.now_us);
                self.opened = self.db.clone();
                if new {
                    // `--new` empties the file even if nothing is written afterwards.
                    self.opened = Database::open(&[0xff]).unwrap_or_default();
                }
            }
            Err(e) => {
                self.db = Database::new();
                self.opened = self.db.clone();
                self.open_error = Some(e);
            }
        }
        self.db.set_now(self.now_us);
    }
    /// Write the database back if it changed. A database that was only read is left
    /// alone, and a new one is only created once something is stored in it.
    fn flush_db(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.open_error.is_some() {
            return;
        }
        self.db.rollback_all();
        if !self.db.same_content(&self.opened) {
            let bytes = self.db.to_bytes();
            if let Err(e) = self.host.write(&path, &bytes) {
                self.error(format!("Error: unable to write {path}: {e}"));
            }
            self.opened = self.db.clone();
        }
    }
    fn finish(mut self) -> CliResult {
        self.flush_db();
        self.flush_redirect();
        CliResult {
            stdout: self.out,
            stderr: self.err,
            code: self.quit.unwrap_or(if self.errors > 0 { 1 } else { 0 }),
        }
    }
    /// Write what `.output`/`.once` captured to its file (which the redirect truncated).
    fn flush_redirect(&mut self) {
        if let Some((path, _, buf)) = self.redirect.take() {
            if let Err(e) = self.host.write(&path, buf.as_bytes()) {
                self.error(format!("Error: cannot write \"{path}\": {e}"));
            }
        }
    }
    /// One command-line argument: a dot-command or SQL. Returns false to stop.
    fn argument(&mut self, text: &str) -> bool {
        if self.echo {
            let t = format!("{text}\n");
            self.print(&t);
        }
        let before = self.errors;
        if text.trim_start().starts_with('.') {
            self.dot(text.trim());
        } else {
            self.sql(text, None);
        }
        // An error on the command line ends the run, as sqlite3 does.
        self.errors == before
    }
    /// Script input (stdin, `.read`): dot-commands on their own lines, SQL until a
    /// statement is complete.
    fn script(&mut self, text: &str) {
        let mut buffer = String::new();
        let mut start_line = 0;
        for (n, line) in text.split_inclusive('\n').enumerate() {
            if self.quit.is_some() || (self.bail && self.errors > 0) {
                return;
            }
            if buffer.trim().is_empty() && line.trim_start().starts_with('.') {
                buffer.clear();
                if self.echo {
                    let t = format!("{}\n", line.trim_end());
                    self.print(&t);
                }
                self.dot(line.trim());
                continue;
            }
            if buffer.trim().is_empty() {
                buffer.clear();
                start_line = n + 1;
            }
            buffer.push_str(line);
            if complete(&buffer) {
                let sql = std::mem::take(&mut buffer);
                if self.echo {
                    let t = format!("{}\n", sql.trim_end());
                    self.print(&t);
                }
                self.sql(&sql, Some(start_line));
            }
        }
        if !buffer.trim().is_empty() && self.quit.is_none() {
            self.sql(&buffer, Some(start_line));
        }
    }
    fn sql(&mut self, text: &str, line: Option<usize>) {
        if let Some(e) = self.open_error.clone() {
            self.report(&e, text, 0, line, false);
            return;
        }
        for (start, stmt) in crate::lexer::split_statements(text) {
            if crate::lexer::is_blank(&stmt) {
                continue;
            }
            let stmt_line =
                line.map(|l| l + text[..start].matches('\n').count() + leading_newlines(&stmt));
            match self.db.execute_one(&stmt, &[]) {
                Ok(o) => {
                    self.render(&o);
                    if self.changes && o.columns.is_empty() {
                        let t = format!(
                            "changes: {}   total_changes: {}\n",
                            self.db.changes(),
                            self.db.total_changes_count()
                        );
                        self.print(&t);
                    }
                    if let Some((_, true, _)) = &self.redirect {
                        self.flush_redirect();
                    }
                }
                Err(e) => {
                    let runtime = e.code != 1 || is_runtime(&e.message);
                    self.report(&e, &stmt, 0, stmt_line, runtime);
                    if self.bail || line.is_none() {
                        return;
                    }
                }
            }
        }
    }
    fn report(
        &mut self,
        e: &SqlError,
        stmt: &str,
        _base: usize,
        line: Option<usize>,
        runtime: bool,
    ) {
        let code = if e.code != 1 {
            format!(" ({})", e.code)
        } else {
            String::new()
        };
        let mut msg = match (line, runtime) {
            (Some(l), false) => format!("Parse error near line {l}: {}{code}", e.message),
            (Some(l), true) => format!("Runtime error near line {l}: {}{code}", e.message),
            (None, false) => format!("Error: in prepare, {}{code}", e.message),
            (None, true) => format!("Error: stepping, {}{code}", e.message),
        };
        if let Some(off) = e.offset {
            // Show the offending line with a caret, as the shell does for syntax errors.
            let text = stmt.trim_start();
            let skipped = stmt.len() - text.len();
            let off = off.saturating_sub(skipped).min(text.len());
            let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
            let line_end = text[off..].find('\n').map_or(text.len(), |i| off + i);
            let shown = text[line_start..line_end].trim_end();
            let col = text[line_start..off].chars().count();
            // The shell points from the left near the start and from the right beyond.
            let pointer = if col < 25 {
                format!("{}^--- error here", " ".repeat(col))
            } else {
                format!("{}error here ---^", " ".repeat(col - 14))
            };
            msg.push_str(&format!("\n  {shown}\n  {pointer}"));
        }
        self.error(msg);
    }
    fn render(&mut self, o: &Output) {
        if o.columns.is_empty() {
            return;
        }
        if o.plan {
            let mut s = String::from("QUERY PLAN\n");
            let rows: Vec<(i64, i64, String)> = o
                .rows
                .iter()
                .map(|r| {
                    (
                        r[0].to_i64().unwrap_or(0),
                        r[1].to_i64().unwrap_or(0),
                        r[3].to_text(),
                    )
                })
                .collect();
            fn walk(rows: &[(i64, i64, String)], parent: i64, prefix: &str, out: &mut String) {
                let kids: Vec<&(i64, i64, String)> =
                    rows.iter().filter(|r| r.1 == parent).collect();
                for (i, k) in kids.iter().enumerate() {
                    let last = i + 1 == kids.len();
                    out.push_str(&format!(
                        "{prefix}{}{}\n",
                        if last { "`--" } else { "|--" },
                        k.2
                    ));
                    let deeper = format!("{prefix}{}", if last { "   " } else { "|  " });
                    walk(rows, k.0, &deeper, out);
                }
            }
            walk(&rows, 0, "", &mut s);
            self.print(&s);
            return;
        }
        // Text is printed as a C string, so it ends at an embedded NUL.
        let text = |v: &Value, null: &str| -> String {
            match v {
                Value::Null => null.to_owned(),
                other => {
                    let t = other.to_text();
                    match t.find('\0') {
                        Some(i) => t[..i].to_owned(),
                        None => t,
                    }
                }
            }
        };
        let csv = |field: &str, sep: &str| -> String {
            // The shell quotes anything empty, holding the separator, or holding a byte
            // outside printable ASCII or one of space, quote, apostrophe and comma.
            let needs = field.is_empty()
                || field.contains(sep)
                || field
                    .bytes()
                    .any(|b| b <= 0x20 || b >= 0x7f || matches!(b, b'"' | b'\'' | b','));
            if needs {
                format!("\"{}\"", field.replace('"', "\"\""))
            } else {
                field.to_owned()
            }
        };
        let mut s = String::new();
        match self.mode {
            Mode::List | Mode::Tabs | Mode::Ascii => {
                if self.headers {
                    s.push_str(&o.columns.join(&self.colsep));
                    s.push_str(&self.rowsep);
                }
                for r in &o.rows {
                    let cells: Vec<String> = r.iter().map(|v| text(v, &self.nullvalue)).collect();
                    s.push_str(&cells.join(&self.colsep));
                    s.push_str(&self.rowsep);
                }
            }
            Mode::Csv => {
                if self.headers {
                    let h: Vec<String> = o.columns.iter().map(|c| csv(c, &self.colsep)).collect();
                    s.push_str(&h.join(&self.colsep));
                    s.push_str(&self.rowsep);
                }
                for r in &o.rows {
                    let cells: Vec<String> = r
                        .iter()
                        .map(|v| match v {
                            Value::Null => self.nullvalue.clone(),
                            other => csv(&other.to_text(), &self.colsep),
                        })
                        .collect();
                    s.push_str(&cells.join(&self.colsep));
                    s.push_str(&self.rowsep);
                }
            }
            Mode::Quote => {
                if self.headers {
                    let h: Vec<String> = o
                        .columns
                        .iter()
                        .map(|c| Value::Text(c.clone()).quoted())
                        .collect();
                    s.push_str(&h.join(","));
                    s.push('\n');
                }
                for r in &o.rows {
                    let cells: Vec<String> = r.iter().map(Value::quoted).collect();
                    s.push_str(&cells.join(","));
                    s.push('\n');
                }
            }
            Mode::Insert => {
                let cols: Vec<String> = o.columns.iter().map(|c| quote_ident(c)).collect();
                let head = if self.headers {
                    format!("({})", cols.join(","))
                } else {
                    String::new()
                };
                for r in &o.rows {
                    let cells: Vec<String> = r.iter().map(Value::quoted).collect();
                    s.push_str(&format!(
                        "INSERT INTO {}{head} VALUES({});\n",
                        quote_ident(&self.insert_table),
                        cells.join(",")
                    ));
                }
            }
            Mode::Line => {
                let w = o.columns.iter().map(|c| width(c)).max().unwrap_or(0).max(5);
                for (i, r) in o.rows.iter().enumerate() {
                    if i > 0 {
                        s.push('\n');
                    }
                    for (c, v) in o.columns.iter().zip(r) {
                        s.push_str(&format!(
                            "{:>w$} = {}\n",
                            c,
                            text(v, &self.nullvalue),
                            w = w
                        ));
                    }
                }
            }
            Mode::Json => {
                s.push('[');
                for (i, r) in o.rows.iter().enumerate() {
                    if i > 0 {
                        s.push_str(",\n");
                    }
                    s.push('{');
                    for (j, (c, v)) in o.columns.iter().zip(r).enumerate() {
                        if j > 0 {
                            s.push(',');
                        }
                        s.push_str(&json_string(c));
                        s.push(':');
                        s.push_str(&match v {
                            Value::Null => "null".into(),
                            Value::Integer(n) => n.to_string(),
                            Value::Real(r) => format_real(*r),
                            Value::Text(t) => json_string(t),
                            Value::Blob(b) => format!("\"{}\"", crate::value::hex(b)),
                        });
                    }
                    s.push('}');
                }
                s.push_str("]\n");
            }
            Mode::Html => {
                if self.headers {
                    s.push_str("<TR>");
                    for c in &o.columns {
                        s.push_str(&format!("<TH>{}</TH>\n", html(c)));
                    }
                    s.push_str("</TR>\n");
                }
                for r in &o.rows {
                    s.push_str("<TR>");
                    for v in r {
                        s.push_str(&format!("<TD>{}</TD>\n", html(&text(v, &self.nullvalue))));
                    }
                    s.push_str("</TR>\n");
                }
            }
            Mode::Column | Mode::Table | Mode::Box | Mode::Markdown => {
                let cells: Vec<Vec<String>> = o
                    .rows
                    .iter()
                    .map(|r| r.iter().map(|v| text(v, &self.nullvalue)).collect())
                    .collect();
                let show_header = self.headers || self.mode != Mode::Column;
                let mut widths: Vec<usize> = o
                    .columns
                    .iter()
                    .map(|c| if show_header { width(c) } else { 0 })
                    .collect();
                for r in &cells {
                    for (i, c) in r.iter().enumerate() {
                        widths[i] = widths[i].max(width(c));
                    }
                }
                for (i, w) in self.widths.iter().enumerate() {
                    if *w > 0 && i < widths.len() {
                        widths[i] = widths[i].max(*w);
                    }
                }
                let pad =
                    |t: &str, w: usize| format!("{t}{}", " ".repeat(w.saturating_sub(width(t))));
                let center = |t: &str, w: usize| {
                    let total = w.saturating_sub(width(t));
                    let left = total / 2;
                    format!("{}{t}{}", " ".repeat(left), " ".repeat(total - left))
                };
                match self.mode {
                    Mode::Column => {
                        let line = |parts: Vec<String>| parts.join("  ") + "\n";
                        if show_header {
                            s.push_str(&line(
                                o.columns
                                    .iter()
                                    .zip(&widths)
                                    .map(|(c, w)| pad(c, *w))
                                    .collect(),
                            ));
                            s.push_str(&line(widths.iter().map(|w| "-".repeat(*w)).collect()));
                        }
                        for r in &cells {
                            s.push_str(&line(
                                r.iter().zip(&widths).map(|(c, w)| pad(c, *w)).collect(),
                            ));
                        }
                    }
                    Mode::Table | Mode::Markdown => {
                        let md = self.mode == Mode::Markdown;
                        let rule = |corner: &str| {
                            format!(
                                "{corner}{}{corner}\n",
                                widths
                                    .iter()
                                    .map(|w| "-".repeat(w + 2))
                                    .collect::<Vec<_>>()
                                    .join(corner)
                            )
                        };
                        let row = |parts: Vec<String>| format!("| {} |\n", parts.join(" | "));
                        if !md {
                            s.push_str(&rule("+"));
                        }
                        s.push_str(&row(o
                            .columns
                            .iter()
                            .zip(&widths)
                            .map(|(c, w)| center(c, *w))
                            .collect()));
                        s.push_str(&rule(if md { "|" } else { "+" }));
                        for r in &cells {
                            s.push_str(&row(r
                                .iter()
                                .zip(&widths)
                                .map(|(c, w)| pad(c, *w))
                                .collect()));
                        }
                        if !md && !cells.is_empty() {
                            s.push_str(&rule("+"));
                        }
                    }
                    _ => {
                        let rule = |l: &str, m: &str, r: &str| {
                            format!(
                                "{l}{}{r}\n",
                                widths
                                    .iter()
                                    .map(|w| "─".repeat(w + 2))
                                    .collect::<Vec<_>>()
                                    .join(m)
                            )
                        };
                        let row = |parts: Vec<String>| format!("│ {} │\n", parts.join(" │ "));
                        s.push_str(&rule("┌", "┬", "┐"));
                        s.push_str(&row(o
                            .columns
                            .iter()
                            .zip(&widths)
                            .map(|(c, w)| center(c, *w))
                            .collect()));
                        s.push_str(&rule("├", "┼", "┤"));
                        for r in &cells {
                            s.push_str(&row(r
                                .iter()
                                .zip(&widths)
                                .map(|(c, w)| pad(c, *w))
                                .collect()));
                        }
                        s.push_str(&rule("└", "┴", "┘"));
                    }
                }
            }
        }
        self.print(&s);
    }
    fn on_off(&mut self, arg: Option<&str>, usage: &str) -> Option<bool> {
        match arg.map(str::to_ascii_lowercase).as_deref() {
            Some("on" | "yes" | "true" | "1") => Some(true),
            Some("off" | "no" | "false" | "0") => Some(false),
            _ => {
                self.error(format!("Usage: {usage}"));
                None
            }
        }
    }
    fn dot(&mut self, line: &str) {
        let args = split_args(&line[1..]);
        let Some(cmd) = args.first().cloned() else {
            return;
        };
        let arg = |i: usize| args.get(i).map(String::as_str);
        match cmd.as_str() {
            "bail" => {
                if let Some(b) = self.on_off(arg(1), ".bail on|off") {
                    self.bail = b;
                }
            }
            "changes" => {
                if let Some(b) = self.on_off(arg(1), ".changes on|off") {
                    self.changes = b;
                }
            }
            "echo" => {
                if let Some(b) = self.on_off(arg(1), ".echo on|off") {
                    self.echo = b;
                }
            }
            "headers" | "header" => {
                if let Some(b) = self.on_off(arg(1), ".headers on|off") {
                    self.headers = b;
                    self.headers_set = true;
                }
            }
            "help" => {
                self.print(HELP);
            }
            "quit" | "q" => self.quit = Some(if self.errors > 0 { 1 } else { 0 }),
            "exit" => {
                self.quit = Some(arg(1).and_then(|c| c.parse().ok()).unwrap_or(0));
            }
            "mode" => match arg(1) {
                None => {
                    let t = format!("current output mode: {}\n", self.mode.name());
                    self.print(&t);
                }
                Some(m) => match Mode::parse(m) {
                    Some(mode) => {
                        self.set_mode(mode);
                        if mode == Mode::Insert {
                            self.insert_table = arg(2).unwrap_or("table").to_owned();
                        }
                    }
                    None => self.error(
                        "Error: mode should be one of: ascii box column csv html insert json line list markdown quote table tabs",
                    ),
                },
            },
            "nullvalue" => match arg(1) {
                Some(v) => self.nullvalue = unescape(v),
                None => self.error("Usage: .nullvalue STRING"),
            },
            "separator" => match arg(1) {
                Some(c) => {
                    self.colsep = unescape(c);
                    if let Some(r) = arg(2) {
                        self.rowsep = unescape(r);
                    }
                }
                None => self.error("Usage: .separator COL ?ROW?"),
            },
            "width" => {
                self.widths = args[1..].iter().map(|w| w.parse().unwrap_or(0)).collect();
            }
            "print" => {
                let t = format!("{}\n", args[1..].join(" "));
                self.print(&t);
            }
            "show" => {
                let q = |s: &str| format!("\"{}\"", s.replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"));
                let t = format!(
                    "        echo: {}\n         eqp: off\n     explain: auto\n     headers: {}\n        mode: {}\n   nullvalue: {}\n      output: {}\ncolseparator: {}\nrowseparator: {}\n       stats: off\n       width: {}\n    filename: {}\n",
                    if self.echo { "on" } else { "off" },
                    if self.headers { "on" } else { "off" },
                    self.mode.name(),
                    q(&self.nullvalue),
                    self.redirect.as_ref().map_or("stdout".to_string(), |r| r.0.clone()),
                    q(&self.colsep),
                    q(&self.rowsep),
                    self.widths.iter().map(|w| format!("{w} ")).collect::<String>(),
                    self.path.clone().unwrap_or_else(|| ":memory:".into()),
                );
                self.print(&t);
            }
            "databases" => {
                let t = format!("main: {} r/w\n", self.path.clone().unwrap_or_default());
                self.print(&t);
            }
            "tables" | "indexes" | "indices" => self.list_names(&cmd, arg(1)),
            "schema" | "fullschema" => self.schema(arg(1)),
            "dump" => self.dump(arg(1)),
            "import" => self.import(&args[1..]),
            "read" => match arg(1) {
                Some(path) => match self.host.read(path) {
                    Ok(Some(b)) => {
                        let text = String::from_utf8_lossy(&b).into_owned();
                        self.script(&text);
                    }
                    Ok(None) => self.error(format!("Error: cannot open \"{path}\"")),
                    Err(e) => self.error(format!("Error: cannot open \"{path}\": {e}")),
                },
                None => self.error("Usage: .read FILE"),
            },
            "open" => {
                let new = args.iter().any(|a| a == "--new");
                match args[1..].iter().find(|a| !a.starts_with("--")) {
                    Some(p) => {
                        let p = p.clone();
                        self.open(&p, new);
                    }
                    None => {
                        self.flush_db();
                        self.path = None;
                        self.db = Database::new();
                        self.db.set_now(self.now_us);
                        self.opened = self.db.clone();
                    }
                }
            }
            "save" | "backup" => match args[1..].iter().rfind(|a| !a.starts_with("--")) {
                Some(p) => {
                    let bytes = self.db.to_bytes();
                    if let Err(e) = self.host.write(p, &bytes) {
                        self.error(format!("Error: cannot write \"{p}\": {e}"));
                    }
                }
                None => self.error("Usage: .save FILE"),
            },
            "output" | "once" => {
                self.flush_redirect();
                if let Some(p) = arg(1) {
                    self.redirect = Some((p.to_owned(), cmd == "once", String::new()));
                }
            }
            other => {
                self.error(format!(
                    "Error: unknown command or invalid arguments:  \"{other}\". Enter \".help\" for help"
                ));
            }
        }
    }
    fn list_names(&mut self, cmd: &str, pattern: Option<&str>) {
        let pat = pattern.unwrap_or("%");
        let mut names: Vec<String> = self
            .db
            .schema()
            .into_iter()
            .filter(|e| {
                if cmd == "tables" {
                    (e.kind == "table" || e.kind == "view")
                        && !e.name.starts_with("sqlite_")
                        && crate::eval::like(pat, &e.name, None)
                } else {
                    e.kind == "index" && crate::eval::like(pat, &e.table, None)
                }
            })
            .map(|e| e.name)
            .collect();
        names.sort();
        if names.is_empty() {
            return;
        }
        let max = names.iter().map(|n| width(n)).max().unwrap_or(0);
        let cols = (80 / (max + 2)).max(1);
        let rows = names.len().div_ceil(cols);
        let mut s = String::new();
        for r in 0..rows {
            let mut i = r;
            let mut first = true;
            while i < names.len() {
                if !first {
                    s.push_str("  ");
                }
                s.push_str(&format!("{:<w$}", names[i], w = max));
                first = false;
                i += rows;
            }
            s.push('\n');
        }
        self.print(&s);
    }
    fn schema_text(sql: &str) -> String {
        // The shell reprints quoted table names with IF NOT EXISTS.
        if let Some(rest) = sql.strip_prefix("CREATE TABLE ") {
            if rest.starts_with('"') || rest.starts_with('\'') {
                return format!("CREATE TABLE IF NOT EXISTS {rest}");
            }
        }
        sql.to_owned()
    }
    fn schema(&mut self, pattern: Option<&str>) {
        let mut s = String::new();
        for e in self.db.schema() {
            let Some(sql) = e.sql else {
                continue;
            };
            if let Some(p) = pattern {
                if !crate::eval::like(p, &e.table, None) && !crate::eval::like(p, &e.name, None) {
                    continue;
                }
            }
            s.push_str(&Self::schema_text(&sql));
            s.push_str(";\n");
        }
        self.print(&s);
    }
    fn dump(&mut self, pattern: Option<&str>) {
        let mut s = String::from("PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\n");
        let schema = self.db.schema();
        let mut sequence = None;
        for e in schema.iter().filter(|e| e.kind == "table") {
            if pattern.is_some_and(|p| !crate::eval::like(p, &e.name, None)) {
                continue;
            }
            if e.name.eq_ignore_ascii_case("sqlite_sequence") {
                sequence = Some(e.clone());
                continue;
            }
            if let Some(sql) = &e.sql {
                s.push_str(&Self::schema_text(sql));
                s.push_str(";\n");
            }
            let mut scratch = self.db.clone();
            if let Ok(o) =
                scratch.execute_one(&format!("SELECT * FROM {}", quote_ident(&e.name)), &[])
            {
                for r in o.rows {
                    let vals: Vec<String> = r.iter().map(Value::quoted).collect();
                    s.push_str(&format!(
                        "INSERT INTO {} VALUES({});\n",
                        quote_ident(&e.name),
                        vals.join(",")
                    ));
                }
            }
        }
        if let Some(seq) = sequence {
            s.push_str("DELETE FROM sqlite_sequence;\n");
            let mut scratch = self.db.clone();
            if let Ok(o) = scratch.execute_one(&format!("SELECT * FROM {}", seq.name), &[]) {
                for r in o.rows {
                    let vals: Vec<String> = r.iter().map(Value::quoted).collect();
                    s.push_str(&format!(
                        "INSERT INTO sqlite_sequence VALUES({});\n",
                        vals.join(",")
                    ));
                }
            }
        }
        for e in schema.iter().filter(|e| e.kind != "table") {
            if pattern.is_some_and(|p| !crate::eval::like(p, &e.table, None)) {
                continue;
            }
            if let Some(sql) = &e.sql {
                s.push_str(sql);
                s.push_str(";\n");
            }
        }
        s.push_str("COMMIT;\n");
        self.print(&s);
    }
    fn import(&mut self, args: &[String]) {
        let mut csv = self.mode == Mode::Csv;
        let mut skip = 0usize;
        let mut rest = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--csv" => csv = true,
                "--ascii" => csv = false,
                "--skip" => {
                    i += 1;
                    skip = args.get(i).and_then(|n| n.parse().ok()).unwrap_or(0);
                }
                "--schema" => i += 1,
                "-v" => {}
                o if o.starts_with('-') => {
                    self.error(format!("Error: unknown option: {o}"));
                    return;
                }
                other => rest.push(other.to_owned()),
            }
            i += 1;
        }
        let [path, table] = rest.as_slice() else {
            self.error("Usage: .import FILE TABLE");
            return;
        };
        let text = match self.host.read(path) {
            Ok(Some(b)) => String::from_utf8_lossy(&b).into_owned(),
            _ => {
                self.error(format!("Error: cannot open \"{path}\""));
                return;
            }
        };
        let sep = if csv {
            ','
        } else if self.mode == Mode::Ascii {
            '\u{1f}'
        } else {
            self.colsep.chars().next().unwrap_or('|')
        };
        let mut rows = crate::csv::parse(&text, sep);
        let mut line = skip + 1;
        rows.drain(..skip.min(rows.len()));
        let exists = self.db.table_info(table).is_some();
        let columns = if exists {
            self.db.table_info(table).map_or(0, |c| c.len())
        } else {
            let Some(header) = (!rows.is_empty()).then(|| rows.remove(0)) else {
                return;
            };
            line += 1;
            let cols: Vec<String> = header
                .iter()
                .map(|h| format!("\"{}\" TEXT", h.replace('"', "\"\"")))
                .collect();
            let sql = format!(
                "CREATE TABLE \"{}\"(\n{})",
                table.replace('"', "\"\""),
                cols.join(", ")
            );
            if let Err(e) = self.db.execute_one(&sql, &[]) {
                self.error(format!("Error: {e}"));
                return;
            }
            header.len()
        };
        let placeholders = vec!["?"; columns].join(",");
        let insert = format!("INSERT INTO {} VALUES({placeholders})", quote_ident(table));
        let _ = self.db.execute_one("SAVEPOINT import", &[]);
        for (n, r) in rows.into_iter().enumerate() {
            let at = line + n;
            let mut vals: Vec<Value> = r.iter().map(|f| Value::Text(f.clone())).collect();
            if vals.len() < columns {
                self.err.push_str(&format!(
                    "{path}:{at}: expected {columns} columns but found {} - filling the rest with NULL\n",
                    vals.len()
                ));
                vals.resize(columns, Value::Null);
            } else if vals.len() > columns {
                self.err.push_str(&format!(
                    "{path}:{at}: expected {columns} columns but found {} - extras ignored\n",
                    vals.len()
                ));
                vals.truncate(columns);
            }
            if let Err(e) = self.db.execute_one(&insert, &vals) {
                self.error(format!("{path}:{at}: INSERT failed: {e}"));
            }
        }
        let _ = self.db.execute_one("RELEASE import", &[]);
    }
}
fn is_runtime(message: &str) -> bool {
    message.contains("constraint failed")
        || message.starts_with("integer overflow")
        || message.starts_with("cannot commit")
        || message.starts_with("cannot rollback")
        || message.starts_with("cannot start a transaction")
        || message.starts_with("datatype mismatch")
        || message.contains("sub-select returns")
        || message.starts_with("too many levels")
        || message.starts_with("error in trigger")
}
fn leading_newlines(s: &str) -> usize {
    let trimmed = s.trim_start();
    s[..s.len() - trimmed.len()].matches('\n').count()
}
/// Whether buffered input ends with a complete statement (`sqlite3_complete`).
fn complete(buffer: &str) -> bool {
    crate::lexer::is_complete(buffer)
}
fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
