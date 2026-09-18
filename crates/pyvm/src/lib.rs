//! `cw-pyvm`: a deterministic Python 3 interpreter for simulated computers.
//!
//! Source is tokenized (INDENT/DEDENT), parsed to an AST, compiled to bytecode
//! with CPython's scoping rules, and run by a VM whose Python-to-Python calls
//! live on a heap frame stack. Files, time and entropy come only from the
//! [`ScriptHost`]; there is no host I/O. Output, tracebacks, error messages and
//! exit statuses follow CPython 3.12.
//!
//! Every run has a step budget ([`vm::STEP_BUDGET`] bytecode instructions). A
//! program that exhausts it is stopped with a traceback ending in
//! `TimeoutError: execution step limit exceeded …` and exit status 124, the
//! status `timeout(1)` uses, so infinite loops terminate deterministically.
pub mod ast;
pub mod bfuncs;
pub mod bigint;
pub mod builtins;
pub mod compiler;
pub mod format;
pub mod io;
pub mod lexer;
pub mod methods;
pub mod modules;
pub mod ops;
pub mod parser;
pub mod value;
pub mod vm;

use cw_script_host::{Invocation, Outcome, ScriptHost};
use std::cell::RefCell;
use std::rc::Rc;
use value::*;
use vm::*;

pub const VERSION: &str = "Python 3.12.3";
pub const TIMEOUT_EXIT: i32 = 124;

const USAGE: &str = "usage: python3 [option] ... [-c cmd | -m mod | file | -] [arg] ...\n";

/// Compiles source to a code object, raising SyntaxError on failure.
pub fn compile_source(vm: &mut Vm, src: &str, filename: &str, mode: &str) -> PyResult<Rc<Code>> {
    let result = parser::parse_module(src).and_then(|(body, warnings)| {
        for w in warnings {
            let line = src
                .lines()
                .nth(w.line.saturating_sub(1) as usize)
                .unwrap_or("");
            let shown = if filename.starts_with('<') {
                String::new()
            } else {
                format!("  {}\n", line.trim())
            };
            vm.write_stderr(&format!(
                "{filename}:{}: SyntaxWarning: {}\n{shown}",
                w.line, w.msg
            ));
        }
        compiler::compile_module(&body, filename, src, mode)
    });
    result.map_err(|e| syntax_error(vm, &e, src, filename))
}

fn syntax_error(vm: &mut Vm, e: &lexer::SyntaxErr, src: &str, filename: &str) -> Box<PyErr> {
    let text = src
        .lines()
        .nth(e.line.saturating_sub(1) as usize)
        .map(|l| Value::string(format!("{l}\n")))
        .unwrap_or(Value::None);
    let details = Value::tuple(vec![
        Value::str(filename),
        Value::Int(e.line as i64),
        Value::Int(e.col as i64),
        text,
        Value::Int(e.line as i64),
        Value::Int(e.end_col as i64),
    ]);
    let _ = vm;
    err_args(e.kind, vec![Value::str(&e.msg), details])
}

impl<'h> Vm<'h> {
    pub fn new(
        host: &'h mut dyn ScriptHost,
        argv: Vec<String>,
        env: Vec<(String, String)>,
        stdin: String,
    ) -> Self {
        let t = builtins::make_types();
        let mut vm = Vm {
            host,
            t,
            builtins: new_ref(Dict::new()),
            modules: new_ref(Dict::new()),
            stdout: String::new(),
            stderr: String::new(),
            stdin,
            stdin_pos: 0,
            fuel: STEP_BUDGET,
            depth: 0,
            native_depth: 0,
            recursion_limit: 1000,
            exc_stack: vec![],
            frames: vec![],
            argv,
            env,
            int_max_str_digits: 4300,
            time_offset: 0,
            repr_guard: vec![],
            script_dir: String::new(),
            sources: Default::default(),
            warnings: vec![],
            set_finger: 0,
            output_limit: 16 << 20,
            classes: vec![],
            id_map: RefCell::new(Default::default()),
            open_files: vec![],
            call_sites: vec![],
        };
        bfuncs::install(&mut vm);
        methods::install(&mut vm);
        let bmod = Rc::new(Module {
            name: "builtins".into(),
            dict: vm.builtins.clone(),
        });
        vm.modules
            .borrow_mut()
            .set_str("builtins", Value::Module(bmod));
        vm
    }

    /// Breaks the reference cycles a run leaves behind (module and class dicts,
    /// function globals) so the interpreter's heap is freed after each command.
    pub fn teardown(&mut self) {
        let mods: Vec<Value> = self.modules.borrow().values();
        for m in mods {
            if let Value::Module(m) = m {
                m.dict.borrow_mut().clear();
            }
        }
        self.modules.borrow_mut().clear();
        for c in std::mem::take(&mut self.classes) {
            if let Some(c) = c.upgrade() {
                c.dict.borrow_mut().clear();
                c.mro.borrow_mut().clear();
                c.bases.borrow_mut().clear();
            }
        }
        self.id_map.borrow_mut().clear();
        self.builtins.borrow_mut().clear();
        self.exc_stack.clear();
    }

    // ------------------------------------------------------------------
    // Builtins that need the calling frame
    // ------------------------------------------------------------------

    pub fn frame_locals(&mut self, f: &Frame) -> Value {
        if let Some(l) = &f.locals {
            return Value::Dict(l.clone());
        }
        if f.code.uses_name_ops {
            return Value::Dict(f.globals.clone());
        }
        let mut d = Dict::new();
        for (i, name) in f.code.varnames.iter().enumerate() {
            if !f.fast[i].is_undefined() {
                d.set_str(name, f.fast[i].clone());
            }
        }
        let nc = f.code.cellvars.len();
        for (i, name) in f
            .code
            .cellvars
            .iter()
            .chain(f.code.freevars.iter())
            .enumerate()
        {
            if i >= nc && &**name == "__class__" {
                continue;
            }
            if let Some(c) = f.cells.get(i) {
                let v = c.borrow().clone();
                if !v.is_undefined() {
                    d.set_str(name, v);
                }
            }
        }
        Value::dict(d)
    }

    /// Handles `globals()`, `locals()`, `vars()`, `dir()`, `exec`, `eval` and
    /// zero-argument `super()`, which read the caller's frame.
    pub fn frame_builtin(
        &mut self,
        f: &mut Frame,
        name: &str,
        args: &[Value],
        kwargs: &[(Rc<str>, Value)],
    ) -> Option<PyResult<Value>> {
        match (name, args.len()) {
            ("globals", 0) => Some(Ok(Value::Dict(f.globals.clone()))),
            ("locals", 0) | ("vars", 0) => Some(Ok(self.frame_locals(f))),
            ("dir", 0) => {
                let l = self.frame_locals(f);
                let mut keys: Vec<String> = match &l {
                    Value::Dict(d) => d
                        .borrow()
                        .keys()
                        .iter()
                        .filter_map(|k| k.as_pystr().map(|s| s.s.clone()))
                        .collect(),
                    _ => vec![],
                };
                keys.sort();
                Some(Ok(Value::list(
                    keys.iter().map(|k| Value::str(k)).collect(),
                )))
            }
            ("exec", _) | ("eval", _) => Some(self.do_exec(f, name, args, kwargs)),
            ("super", 0) => Some(self.zero_arg_super(f)),
            _ => None,
        }
    }

    fn zero_arg_super(&mut self, f: &Frame) -> PyResult<Value> {
        let nc = f.code.cellvars.len();
        let cls = f
            .code
            .freevars
            .iter()
            .position(|n| &**n == "__class__")
            .and_then(|i| f.cells.get(nc + i))
            .map(|c| c.borrow().clone());
        let cls = match cls {
            Some(Value::Class(c)) => c,
            _ => return Err(err("RuntimeError", "super(): __class__ cell not found")),
        };
        if f.code.argcount == 0 && !f.code.varargs {
            return Err(err("RuntimeError", "super(): no arguments"));
        }
        let mut first = f.fast.first().cloned().unwrap_or(Value::Undefined);
        if first.is_undefined() {
            // The first argument may live in a cell (captured by a closure).
            if let Some(ci) = f.code.cell2arg.iter().position(|a| *a == Some(0)) {
                first = f.cells[ci].borrow().clone();
            }
        }
        if let (true, Some(Value::Tuple(t))) = (f.code.argcount == 0, f.fast.first()) {
            first = t.first().cloned().unwrap_or(Value::Undefined);
        }
        if first.is_undefined() {
            return Err(err("RuntimeError", "super(): arg[0] deleted"));
        }
        Ok(Value::Super(Rc::new((cls, first))))
    }

    fn do_exec(
        &mut self,
        f: &mut Frame,
        name: &str,
        args: &[Value],
        kwargs: &[(Rc<str>, Value)],
    ) -> PyResult<Value> {
        let mut args = args.to_vec();
        for (k, v) in kwargs {
            match &**k {
                "globals" if args.len() == 1 => args.push(v.clone()),
                "locals" => {
                    while args.len() < 2 {
                        args.push(Value::None);
                    }
                    args.push(v.clone());
                }
                _ => {}
            }
        }
        let Some(src) = args.first().cloned() else {
            return Err(type_err(format!(
                "{name} expected at least 1 argument, got 0"
            )));
        };
        let globals = match args.get(1) {
            Some(Value::Dict(d)) => {
                if !d.borrow().contains_str("__builtins__") {
                    d.borrow_mut()
                        .set_str("__builtins__", Value::Dict(self.builtins.clone()));
                }
                d.clone()
            }
            Some(Value::None) | None => f.globals.clone(),
            Some(other) => {
                return Err(type_err(format!(
                    "{name}() globals must be a dict, not {}",
                    self.type_name(other)
                )))
            }
        };
        let locals = match args.get(2) {
            Some(Value::Dict(d)) => Some(d.clone()),
            Some(Value::None) | None => {
                if args.get(1).is_some_and(|g| !g.is_none()) {
                    Some(globals.clone())
                } else if f.code.uses_name_ops {
                    Some(f.locals.clone().unwrap_or_else(|| f.globals.clone()))
                } else {
                    match self.frame_locals(f) {
                        Value::Dict(d) => Some(d),
                        _ => None,
                    }
                }
            }
            Some(other) => {
                let _ = other;
                return Err(type_err("locals must be a mapping"));
            }
        };
        let code = match &src {
            Value::Str(s) => {
                let text = if name == "eval" {
                    s.s.trim_start_matches([' ', '\t']).to_string()
                } else {
                    s.s.clone()
                };
                compile_source(self, &text, "<string>", name)?
            }
            Value::Code(c) => c.clone(),
            other => {
                return Err(type_err(format!(
                    "{name}() arg 1 must be a string, bytes or code object, not {}",
                    self.type_name(other)
                )))
            }
        };
        let frame = self.new_frame(code, globals.clone(), locals);
        self.charge_depth()?;
        let r = match self.execute(frame, None)? {
            Exit::Return(v) => v,
            Exit::Yield(..) => Value::None,
        };
        Ok(if name == "eval" { r } else { Value::None })
    }
}

// ---------------------------------------------------------------------------
// Tracebacks
// ---------------------------------------------------------------------------

fn class_display_name(vm: &Vm, cls: &Rc<Class>) -> String {
    let module = cls
        .dict
        .borrow()
        .get_str("__module__")
        .and_then(|m| m.as_pystr().map(|s| s.s.clone()))
        .unwrap_or_else(|| "builtins".into());
    let _ = vm;
    let q = cls.qualname.borrow().to_string();
    if module == "builtins" {
        q
    } else {
        format!("{module}.{q}")
    }
}

/// Carets under the failing expression, following CPython 3.12's rules.
fn caret_line(line: &str, pos: &Pos) -> Option<String> {
    if pos.end_col == u32::MAX || pos.col as usize >= line.chars().count() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let stripped_prefix = chars.iter().take_while(|c| c.is_whitespace()).count();
    let stripped_len = line.trim().chars().count();
    let start = pos.col as usize;
    let mut end = if pos.end_line > pos.line {
        line.trim_end().chars().count()
    } else {
        (pos.end_col as usize).min(chars.len())
    };
    if end <= start {
        end = start + 1;
    }
    let segment: String = chars[start..end.min(chars.len())].iter().collect();
    let seg: Vec<char> = segment.chars().collect();
    // Anchors: (left_end, right_start) relative to the segment.
    let mut anchors: Option<(usize, usize, char, char)> = None;
    if pos.end_line == pos.line {
        match pos.anchor.2 {
            1 => {
                // Binary operator: find it between the operands.
                let left_end = (pos.anchor.0 as usize).saturating_sub(start);
                let right_start = (pos.anchor.1 as usize).saturating_sub(start);
                if left_end <= right_start && right_start <= seg.len() {
                    let op: String = seg[left_end..right_start].iter().collect();
                    let off = op
                        .chars()
                        .take_while(|c| c.is_whitespace() || *c == ')')
                        .count();
                    let mut l = left_end + off;
                    let mut r = l + 1;
                    if off + 1 < op.chars().count()
                        && !op.chars().nth(off + 1).is_some_and(|c| c.is_whitespace())
                    {
                        r += 1;
                    }
                    while l < seg.len()
                        && (seg[l].is_whitespace() || seg[l] == ')' || seg[l] == '#')
                    {
                        l += 1;
                        r += 1;
                    }
                    anchors = Some((l, r, '~', '^'));
                }
            }
            2 => {
                let mut l = (pos.anchor.0 as usize).saturating_sub(start);
                let mut r = (pos.anchor.1 as usize + 1).saturating_sub(start);
                while l < seg.len() && seg[l] != '[' {
                    l += 1;
                }
                while r < seg.len() && seg[r] != ']' {
                    r += 1;
                }
                if r < seg.len() {
                    r += 1;
                }
                anchors = Some((l, r, '~', '^'));
            }
            _ => {}
        }
    }
    let width = end - start;
    let show = width < stripped_len || anchors.is_some_and(|(l, r, _, _)| r > l);
    if !show {
        return None;
    }
    let mut out = String::from("    ");
    out.push_str(&" ".repeat(start.saturating_sub(stripped_prefix)));
    match anchors {
        Some((l, r, prim, sec)) => {
            let l = l.min(width);
            let r = r.min(width).max(l);
            out.push_str(&prim.to_string().repeat(l));
            out.push_str(&sec.to_string().repeat(r - l));
            out.push_str(&prim.to_string().repeat(width - r));
        }
        None => out.push_str(&"^".repeat(width)),
    }
    Some(out)
}

pub fn format_traceback_entries(vm: &Vm, tb: &[TbEntry]) -> String {
    let mut out = String::new();
    let mut last: Option<(String, u32, String)> = None;
    let mut repeat = 0;
    let flush_repeat = |out: &mut String, repeat: usize| {
        if repeat > 3 {
            out.push_str(&format!(
                "  [Previous line repeated {} more time{}]\n",
                repeat - 3,
                if repeat - 3 == 1 { "" } else { "s" }
            ));
        }
    };
    for e in tb.iter().rev() {
        let key = (e.filename.to_string(), e.pos.line, e.name.to_string());
        if last.as_ref() == Some(&key) {
            repeat += 1;
            if repeat > 3 {
                continue;
            }
        } else {
            flush_repeat(&mut out, repeat);
            repeat = 1;
            last = Some(key);
        }
        out.push_str(&format!(
            "  File \"{}\", line {}, in {}\n",
            e.filename, e.pos.line, e.name
        ));
        if let Some(src) = vm.sources.get(&*e.filename) {
            if let Some(line) = src.lines().nth(e.pos.line.saturating_sub(1) as usize) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push_str(&format!("    {trimmed}\n"));
                    if let Some(c) = caret_line(line, &e.pos) {
                        out.push_str(&c);
                        out.push('\n');
                    }
                }
            }
        }
    }
    flush_repeat(&mut out, repeat);
    out
}

/// The full report CPython prints for an uncaught exception, chain included.
pub fn format_exception(vm: &mut Vm, exc: &Value, depth: usize) -> String {
    let mut out = String::new();
    if depth < 20 {
        let (cause, context, suppress) = vm
            .exc_data(exc, |d| {
                (d.cause.clone(), d.context.clone(), d.suppress_context)
            })
            .unwrap_or((None, None, false));
        if let Some(c) = cause {
            out.push_str(&format_exception(vm, &c, depth + 1));
            out.push_str(
                "\nThe above exception was the direct cause of the following exception:\n\n",
            );
        } else if let (Some(c), false) = (context, suppress) {
            out.push_str(&format_exception(vm, &c, depth + 1));
            out.push_str(
                "\nDuring handling of the above exception, another exception occurred:\n\n",
            );
        }
    }
    let tb = vm
        .exc_data(exc, |d| d.traceback.clone())
        .unwrap_or_default();
    if !tb.is_empty() {
        out.push_str("Traceback (most recent call last):\n");
        out.push_str(&format_traceback_entries(vm, &tb));
    }
    let cls = vm.type_of(exc);
    let name = class_display_name(vm, &cls);
    if cls.is_subclass(&vm.t.exc("SyntaxError")) {
        let args = builtins::exc_args(vm, exc);
        if let (Some(msg), Some(Value::Tuple(d))) = (args.first(), args.get(1)) {
            let msg = vm.str_of(msg).unwrap_or_default();
            let file = d
                .first()
                .map(|f| vm.str_of(f).unwrap_or_default())
                .unwrap_or_default();
            let line = match d.get(1) {
                Some(Value::Int(l)) => *l,
                _ => 0,
            };
            out.push_str(&format!("  File \"{file}\", line {line}\n"));
            if let Some(Value::Str(text)) = d.get(3) {
                let raw = text.s.trim_end_matches('\n');
                let indent = raw.chars().take_while(|c| c.is_whitespace()).count();
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    out.push_str(&format!("    {trimmed}\n"));
                    let offset = match d.get(2) {
                        Some(Value::Int(o)) => *o as usize,
                        _ => 0,
                    };
                    let end = match d.get(5) {
                        Some(Value::Int(o)) => *o as usize,
                        _ => offset + 1,
                    };
                    if offset >= 1 {
                        let col = offset.saturating_sub(1).saturating_sub(indent);
                        let width = end.saturating_sub(offset).max(1);
                        let width = width.min(trimmed.chars().count().saturating_sub(col).max(1));
                        out.push_str(&format!("    {}{}\n", " ".repeat(col), "^".repeat(width)));
                    }
                }
            }
            out.push_str(&format!("{name}: {msg}\n"));
            return out;
        }
    }
    let msg = match vm.str_of(exc) {
        Ok(s) => s,
        Err(_) => "<exception str() failed>".into(),
    };
    if msg.is_empty() {
        out.push_str(&format!("{name}\n"));
    } else {
        out.push_str(&format!("{name}: {msg}\n"));
    }
    if let Some(Value::List(notes)) = match exc {
        Value::Instance(i) => i.dict.borrow().get_str("__notes__"),
        _ => None,
    } {
        let notes = notes.borrow().clone();
        for n in notes {
            if let Ok(s) = vm.str_of(&n) {
                out.push_str(&format!("{s}\n"));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

enum Target {
    Code(String, String),
    Module(String),
    File(String),
}

/// Runs `python3 <args>` against the host and returns both streams and the status.
pub fn run(host: &mut dyn ScriptHost, invocation: &Invocation) -> Outcome {
    let args = &invocation.args;
    let mut i = 0;
    let mut target = None;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--version" | "-V" => {
                return Outcome {
                    stdout: format!("{VERSION}\n"),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
            "-h" | "--help" | "-?" => {
                return Outcome {
                    stdout: format!(
                        "{USAGE}Options (the simulated interpreter accepts and ignores -B -E -I -O -q -s -S -u -W arg -X opt):\n-c cmd : program passed in as string (terminates option list)\n-m mod : run library module as a script (terminates option list)\n-V     : print the Python version number and exit\nfile   : program read from script file\n-      : program read from stdin (default)\narg ...: arguments passed to program in sys.argv[1:]\n"
                    ),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
            "-c" => {
                let Some(code) = args.get(i + 1) else {
                    return Outcome {
                        stdout: String::new(),
                        stderr: format!("Argument expected for the -c option\n{USAGE}Try `python -h' for more information.\n"),
                        exit_code: 2,
                    };
                };
                target = Some((Target::Code(code.clone(), "-c".into()), i + 2));
                break;
            }
            "-m" => {
                let Some(m) = args.get(i + 1) else {
                    return Outcome {
                        stdout: String::new(),
                        stderr: format!("Argument expected for the -m option\n{USAGE}Try `python -h' for more information.\n"),
                        exit_code: 2,
                    };
                };
                target = Some((Target::Module(m.clone()), i + 2));
                break;
            }
            "-" => {
                target = Some((Target::Code(invocation.stdin.clone(), "-".into()), i + 1));
                break;
            }
            "-W" | "-X" => i += 2,
            _ if a.starts_with('-') && a.len() > 1 => {
                let flags = &a[1..];
                if flags.chars().all(|c| "BEIOqsSuvdbi".contains(c)) {
                    i += 1;
                    continue;
                }
                let bad = flags.chars().find(|c| !"BEIOqsSuvdbi".contains(*c)).unwrap_or('?');
                return Outcome {
                    stdout: String::new(),
                    stderr: format!("Unknown option: -{bad}\n{USAGE}Try `python -h' for more information.\n"),
                    exit_code: 2,
                };
            }
            _ => {
                target = Some((Target::File(a.to_string()), i + 1));
                break;
            }
        }
    }
    let (target, rest) = match target {
        Some(t) => t,
        None => (
            Target::Code(invocation.stdin.clone(), "".into()),
            args.len(),
        ),
    };
    let reads_stdin_as_program =
        matches!(&target, Target::Code(_, flag) if flag.is_empty() || flag == "-");
    let program_args: Vec<String> = args[rest.min(args.len())..].to_vec();
    let stdin = if reads_stdin_as_program {
        String::new()
    } else {
        invocation.stdin.clone()
    };
    let mut argv0 = String::new();
    let (src, filename) =
        match &target {
            Target::Code(code, flag) => {
                argv0 = if flag == "-c" {
                    "-c".into()
                } else if flag == "-" {
                    "-".into()
                } else {
                    String::new()
                };
                (
                    Some(code.clone()),
                    if flag == "-c" {
                        "<string>".to_string()
                    } else {
                        "<stdin>".to_string()
                    },
                )
            }
            Target::File(path) => {
                let abs = host.resolve(path);
                match host.stat(path) {
                    Ok(st) if st.is_dir => {
                        // A directory needs __main__.py.
                        let main = format!("{}/__main__.py", abs.trim_end_matches('/'));
                        match host.read_file(&main) {
                            Ok(b) => {
                                argv0 = path.clone();
                                (Some(String::from_utf8_lossy(&b).into_owned()), main)
                            }
                            Err(_) => return Outcome {
                                stdout: String::new(),
                                stderr: format!(
                                    "/usr/bin/python3: can't find '__main__' module in '{abs}'\n"
                                ),
                                exit_code: 1,
                            },
                        }
                    }
                    _ => match host.read_file(path) {
                        Ok(b) => {
                            argv0 = path.clone();
                            let text = String::from_utf8_lossy(&b).into_owned();
                            (Some(text), abs)
                        }
                        Err(e) => {
                            return Outcome {
                                stdout: String::new(),
                                stderr: format!(
                                    "/usr/bin/python3: can't open file {}: [Errno {}] {}\n",
                                    format::str_repr(&abs),
                                    e.kind.errno(),
                                    e.kind.strerror()
                                ),
                                exit_code: 2,
                            }
                        }
                    },
                }
            }
            Target::Module(m) => {
                argv0 = m.clone();
                (None, String::new())
            }
        };
    let mut argv = vec![argv0];
    argv.extend(program_args);
    let host_ptr = host;
    let mut vm = Vm::new(host_ptr, argv, invocation.env.clone(), stdin);
    let script_dir = match &target {
        Target::File(_) => filename
            .rsplit_once('/')
            .map(|(d, _)| {
                if d.is_empty() {
                    "/".to_string()
                } else {
                    d.to_string()
                }
            })
            .unwrap_or_default(),
        _ => vm.host.cwd(),
    };
    vm.script_dir = script_dir.clone();
    let outcome = run_main(&mut vm, src, &filename, &target);
    io::flush_all(&mut vm);
    let stdout = std::mem::take(&mut vm.stdout);
    let stderr = std::mem::take(&mut vm.stderr);
    vm.teardown();
    Outcome {
        stdout,
        stderr,
        exit_code: outcome,
    }
}

fn run_main(vm: &mut Vm, src: Option<String>, filename: &str, target: &Target) -> i32 {
    // sys.path[0]: the script's directory, or '' (the cwd) for -c and stdin.
    let sys = match modules_sys(vm) {
        Ok(s) => s,
        Err(e) => return report(vm, e),
    };
    if let Value::Module(m) = &sys {
        let mut path = vec![Value::string(match target {
            Target::File(_) => vm.script_dir.clone(),
            _ => String::new(),
        })];
        for (k, v) in vm.env.clone() {
            if k == "PYTHONPATH" {
                for p in v.split(':').filter(|p| !p.is_empty()) {
                    path.push(Value::str(p));
                }
            }
        }
        m.dict.borrow_mut().set_str("path", Value::list(path));
    }
    let main = modules::new_module("__main__");
    main.dict.borrow_mut().set_str(
        "__builtins__",
        Value::Module(Rc::new(Module {
            name: "builtins".into(),
            dict: vm.builtins.clone(),
        })),
    );
    main.dict.borrow_mut().set_str("__package__", Value::None);
    main.dict.borrow_mut().set_str("__spec__", Value::None);
    main.dict.borrow_mut().set_str("__loader__", Value::None);
    let result: PyResult<()> = (|| {
        let (src, filename) = match target {
            Target::Module(name) => {
                let (src, path) = find_module_source(vm, name)?;
                (src, path)
            }
            _ => (src.unwrap_or_default(), filename.to_string()),
        };
        if !filename.starts_with('<') {
            main.dict
                .borrow_mut()
                .set_str("__file__", Value::str(&filename));
        }
        vm.sources.insert(filename.clone(), src.as_str().into());
        vm.modules
            .borrow_mut()
            .set_str("__main__", Value::Module(main.clone()));
        let code = compile_source(vm, &src, &filename, "exec")?;
        let frame = vm.new_frame(code, main.dict.clone(), None);
        vm.charge_depth()?;
        vm.execute(frame, None)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => report(vm, e),
    }
}

fn modules_sys(vm: &mut Vm) -> PyResult<Value> {
    let g = new_ref(Dict::new());
    vm.import("sys", &Value::None, 0, &g)
}

fn find_module_source(vm: &mut Vm, name: &str) -> PyResult<(String, String)> {
    let rel = name.replace('.', "/");
    for dir in [vm.host.cwd()] {
        for candidate in [
            format!("{dir}/{rel}.py"),
            format!("{dir}/{rel}/__main__.py"),
        ] {
            if let Ok(b) = vm.host.read_file(&candidate) {
                if let Value::Module(sys) = modules_sys(vm)? {
                    if let Some(Value::List(argv)) = sys.dict.borrow().get_str("argv") {
                        if let Some(first) = argv.borrow_mut().first_mut() {
                            *first = Value::string(candidate.clone());
                        }
                    }
                }
                return Ok((String::from_utf8_lossy(&b).into_owned(), candidate));
            }
        }
    }
    if let Some((_, src)) = modules::PY_MODULES.iter().find(|(n, _)| *n == name) {
        return Ok((src.to_string(), format!("<frozen {name}>")));
    }
    Err(err_args(
        "ModuleNotFoundError",
        vec![Value::string(format!("No module named {name}"))],
    ))
}

/// Prints an uncaught exception the way CPython does and returns the status.
fn report(vm: &mut Vm, mut e: Box<PyErr>) -> i32 {
    let fatal = e.fatal;
    let exc = vm.materialize(&mut e);
    if !fatal && vm.isinstance(&exc, &vm.t.exc("SystemExit")) {
        let code = builtins::exc_args(vm, &exc);
        return match code.first() {
            None | Some(Value::None) => 0,
            Some(Value::Int(i)) => (*i as i32) & 0xff,
            Some(Value::Bool(b)) => *b as i32,
            Some(other) => {
                let s = vm.str_of(other).unwrap_or_default();
                vm.write_stderr(&format!("{s}\n"));
                1
            }
        };
    }
    if matches!(exc, Value::Module(_) | Value::None) {
        vm.write_stderr("Fatal Python error: lost exception\n");
        return 1;
    }
    let text = format_exception(vm, &exc, 0);
    vm.write_stderr(&text);
    if fatal {
        TIMEOUT_EXIT
    } else {
        1
    }
}

/// Convenience for tests: run source text as `python3 -c`-like main script at
/// `path` with the given stdin.
pub fn run_source(
    host: &mut dyn ScriptHost,
    path: &str,
    src: &str,
    args: &[&str],
    stdin: &str,
) -> Outcome {
    let _ = host.write_file(path, src.as_bytes(), false);
    let mut a = vec![path.to_string()];
    a.extend(args.iter().map(|s| s.to_string()));
    run(
        host,
        &Invocation {
            args: a,
            env: vec![],
            stdin: stdin.to_string(),
        },
    )
}
