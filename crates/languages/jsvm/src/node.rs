//! The Node.js host layer: console, process, timers, the CommonJS / ES
//! module loaders, the event loop and uncaught-exception reporting.

use crate::builtins::str_arg;
use crate::promise::slots;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

pub const BUILTINS: &[&str] = &[
    "assert",
    "assert/strict",
    "buffer",
    "child_process",
    "crypto",
    "events",
    "fs",
    "fs/promises",
    "os",
    "path",
    "path/posix",
    "process",
    "querystring",
    "readline",
    "readline/promises",
    "stream",
    "string_decoder",
    "timers",
    "timers/promises",
    "url",
    "util",
    "util/types",
    "perf_hooks",
    "worker_threads",
    "http",
    "https",
    "net",
    "zlib",
    "tty",
    "v8",
    "vm",
    "module",
    "constants",
    "async_hooks",
    "cluster",
    "dns",
    "dns/promises",
    "diagnostics_channel",
    "test",
    "sys",
];

/// `require('buffer')`: the global `Buffer` and what Node 24 exports beside it.
/// `SlowBuffer` is deprecated but still there, and `jwa` (every JWT library)
/// patches its prototype when it loads.
const BUFFER_MODULE: &str = "'use strict';
const kMaxLength = 2 ** 32;
function SlowBuffer(size) { return Buffer.allocUnsafeSlow(size); }
Object.setPrototypeOf(SlowBuffer.prototype, Uint8Array.prototype);
Object.setPrototypeOf(SlowBuffer, Uint8Array);
module.exports = {
  Buffer, SlowBuffer, kMaxLength, kStringMaxLength: 2 ** 29 - 24, INSPECT_MAX_BYTES: 50,
  constants: { MAX_LENGTH: kMaxLength, MAX_STRING_LENGTH: 2 ** 29 - 24 },
  Blob: globalThis.Blob, File: globalThis.File, atob: globalThis.atob, btoa: globalThis.btoa,
  isUtf8: (b) => { try { new TextDecoder('utf-8', { fatal: true }).decode(b); return true; } catch { return false; } },
  isAscii: (b) => { for (const x of new Uint8Array(b.buffer || b, b.byteOffset || 0, b.byteLength)) if (x > 127) return false; return true; },
  transcode: (b) => Buffer.from(b),
};
";

/// JS sources of built-in modules implemented in JavaScript.
fn js_module_source(name: &str) -> Option<&'static str> {
    Some(match name {
        "events" => include_str!("../js/events.js"),
        "assert" => include_str!("../js/assert.js"),
        "readline" => include_str!("../js/readline.js"),
        "util" | "sys" => include_str!("../js/util.js"),
        "url" => include_str!("../js/url.js"),
        "querystring" => include_str!("../js/querystring.js"),
        "string_decoder" => include_str!("../js/string_decoder.js"),
        "stream" => include_str!("../js/stream.js"),
        "timers/promises" => include_str!("../js/timers_promises.js"),
        "child_process" => include_str!("../js/child_process.js"),
        "dns" => include_str!("../js/dns.js"),
        "dns/promises" => "module.exports = require('dns').promises;",
        "net" => include_str!("../js/net.js"),
        "http" => include_str!("../js/http.js"),
        "https" => include_str!("../js/https.js"),
        "internal/fetch" => include_str!("../js/fetch.js"),
        "internal/intl" => include_str!("../js/intl.js"),
        "internal/httpwire" => include_str!("../js/httpwire.js"),
        "internal/serve" => include_str!("../js/serve.js"),
        "zlib" => include_str!("../js/zlib.js"),
        "worker_threads" => include_str!("../js/worker_threads.js"),
        "tty" => include_str!("../js/tty.js"),
        _ => return None,
    })
}

// ---------------------------------------------------------------- console

fn write_out(vm: &mut Vm, s: &str, err: bool) {
    let indent = " ".repeat(vm.console_indent);
    let text = if indent.is_empty() {
        s.to_string()
    } else {
        s.lines()
            .map(|l| format!("{indent}{l}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let buf = if err { &mut vm.stderr } else { &mut vm.stdout };
    buf.push_str(&text);
    buf.push('\n');
}

fn console_log(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = vm.format_args(&a.args)?;
    write_out(vm, &s, false);
    Ok(Value::Undefined)
}

fn console_error(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = vm.format_args(&a.args)?;
    write_out(vm, &s, true);
    Ok(Value::Undefined)
}

fn console_dir(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.inspect_opts_from(&a.arg(1))?;
    let o = crate::inspect::Opts {
        custom_inspect: false,
        ..o
    };
    let s = vm.inspect(&a.arg(0), &o)?;
    write_out(vm, &s, false);
    Ok(Value::Undefined)
}

fn console_assert(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if a.arg(0).truthy() {
        return Ok(Value::Undefined);
    }
    let rest: Vec<Value> = a.args.iter().skip(1).cloned().collect();
    let s = if rest.is_empty() {
        "Assertion failed".to_string()
    } else if let Value::Str(first) = &rest[0] {
        let mut r = rest.clone();
        r[0] = Value::string(format!("Assertion failed: {}", first.as_str()));
        vm.format_args(&r)?
    } else {
        let mut r = vec![Value::str("Assertion failed:")];
        r.extend(rest);
        vm.format_args(&r)?
    };
    write_out(vm, &s, true);
    Ok(Value::Undefined)
}

fn console_trace(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = vm.format_args(&a.args)?;
    let (frames, _) = vm.stack_frames(true);
    let mut out = format!("Trace: {s}");
    let mut fr = frames;
    vm.append_tail(&mut fr);
    fr.truncate(vm.stack_limit);
    for f in fr {
        out.push_str("\n    at ");
        out.push_str(&f);
    }
    write_out(vm, &out, true);
    Ok(Value::Undefined)
}

fn label_of(vm: &mut Vm, a: &Args) -> JsResult<String> {
    if a.arg(0).is_undefined() {
        Ok("default".into())
    } else {
        vm.to_str(&a.arg(0))
    }
}

fn console_count(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let l = label_of(vm, a)?;
    let n = match vm.console_counts.iter_mut().find(|(k, _)| *k == l) {
        Some((_, n)) => {
            *n += 1;
            *n
        }
        None => {
            vm.console_counts.push((l.clone(), 1));
            1
        }
    };
    write_out(vm, &format!("{l}: {n}"), false);
    Ok(Value::Undefined)
}

fn console_count_reset(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let l = label_of(vm, a)?;
    match vm.console_counts.iter_mut().find(|(k, _)| *k == l) {
        Some((_, n)) => *n = 0,
        None => write_out(
            vm,
            &format!(
                "(node:{}) Warning: Count for '{l}' does not exist",
                vm.host.pid()
            ),
            true,
        ),
    }
    Ok(Value::Undefined)
}

fn fmt_duration(ms: f64) -> String {
    if ms >= 1000.0 {
        let s = ms / 1000.0;
        if s >= 60.0 {
            let m = s / 60.0;
            return format!("{}min", crate::numconv::to_fixed(m, 3));
        }
        return format!("{}s", crate::numconv::to_fixed(s, 3));
    }
    format!("{}ms", crate::numconv::to_fixed(ms, 3))
}

fn console_time(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let l = label_of(vm, a)?;
    if vm.console_timers.iter().any(|(k, _)| *k == l) {
        let pid = vm.host.pid();
        write_out(
            vm,
            &format!("(node:{pid}) Warning: Label '{l}' already exists for console.time()"),
            true,
        );
        return Ok(Value::Undefined);
    }
    let now = vm.perf_now();
    vm.console_timers.push((l, now));
    Ok(Value::Undefined)
}

fn console_time_end(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    time_log_impl(vm, a, true)
}
fn console_time_log(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    time_log_impl(vm, a, false)
}

fn time_log_impl(vm: &mut Vm, a: &mut Args, end: bool) -> JsResult<Value> {
    let l = label_of(vm, a)?;
    let Some(i) = vm.console_timers.iter().position(|(k, _)| *k == l) else {
        let pid = vm.host.pid();
        write_out(
            vm,
            &format!(
                "(node:{pid}) Warning: No such label '{l}' for console.time{}()",
                if end { "End" } else { "Log" }
            ),
            true,
        );
        return Ok(Value::Undefined);
    };
    let start = vm.console_timers[i].1;
    let el = vm.perf_now() - start;
    if end {
        vm.console_timers.remove(i);
    }
    let mut s = format!("{l}: {}", fmt_duration(el));
    if !end && a.args.len() > 1 {
        let extra = vm.format_args(&a.args[1..])?;
        s.push(' ');
        s.push_str(&extra);
    }
    write_out(vm, &s, false);
    Ok(Value::Undefined)
}

fn console_group(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if !a.args.is_empty() {
        let s = vm.format_args(&a.args)?;
        write_out(vm, &s, false);
    }
    vm.console_indent += 2;
    Ok(Value::Undefined)
}

fn console_group_end(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    vm.console_indent = vm.console_indent.saturating_sub(2);
    Ok(Value::Undefined)
}

fn console_table(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let data = a.arg(0);
    let Value::Obj(o) = &data else {
        return console_log(vm, a);
    };
    let cols_filter: Option<Vec<String>> = match a.arg(1) {
        Value::Obj(c) if c.is_array() => {
            let items = vm.iterable_to_vec(&Value::Obj(c))?;
            let mut v = vec![];
            for it in items {
                v.push(vm.to_str(&it)?);
            }
            Some(v)
        }
        _ => None,
    };
    let is_arr = o.is_array();
    let row_keys: Vec<String> = if is_arr {
        (0..vm.length_of(&data)?).map(|i| i.to_string()).collect()
    } else {
        vm.own_enum_keys(o)?.iter().map(|k| k.to_string()).collect()
    };
    let index_header = "(index)".to_string();
    let mut columns: Vec<String> = vec![];
    let mut has_values = false;
    type Row = (String, Vec<(String, String)>, Option<String>);
    let mut rows: Vec<Row> = vec![];
    let fmt_cell = |vm: &mut Vm, v: &Value| -> JsResult<String> {
        let o = crate::inspect::Opts {
            depth: Some(1.0),
            compact: 3,
            break_length: f64::INFINITY,
            ..Default::default()
        };
        vm.inspect(v, &o)
    };
    for rk in &row_keys {
        let v = vm.get(&data, &Key::str(rk))?;
        let mut cells = vec![];
        let mut value = None;
        match &v {
            Value::Obj(ro) if !ro.is_callable() => {
                let keys: Vec<String> = if ro.is_array() {
                    (0..vm.length_of(&v)?).map(|i| i.to_string()).collect()
                } else {
                    vm.own_enum_keys(ro)?
                        .iter()
                        .map(|k| k.to_string())
                        .collect()
                };
                for k in keys {
                    if let Some(f) = &cols_filter {
                        if !f.contains(&k) {
                            continue;
                        }
                    }
                    let cv = vm.get(&v, &Key::str(&k))?;
                    let s = fmt_cell(vm, &cv)?;
                    if !columns.contains(&k) {
                        columns.push(k.clone());
                    }
                    cells.push((k, s));
                }
            }
            _ => {
                has_values = true;
                value = Some(fmt_cell(vm, &v)?);
            }
        }
        rows.push((rk.clone(), cells, value));
    }
    if let Some(f) = &cols_filter {
        columns = f.clone();
    }
    let mut header = vec![index_header];
    header.extend(columns.iter().cloned());
    if has_values {
        header.push("Values".into());
    }
    let mut table: Vec<Vec<String>> = vec![];
    for (rk, cells, value) in &rows {
        let mut line = vec![rk.clone()];
        for c in &columns {
            line.push(
                cells
                    .iter()
                    .find(|(k, _)| k == c)
                    .map(|(_, s)| s.clone())
                    .unwrap_or_default(),
            );
        }
        if has_values {
            line.push(value.clone().unwrap_or_default());
        }
        table.push(line);
    }
    let width = |s: &str| s.chars().count();
    let mut widths: Vec<usize> = header.iter().map(|h| width(h) + 2).collect();
    for r in &table {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(width(c) + 2);
        }
    }
    // Node 24 left-aligns every cell with one space of padding.
    let center = |s: &str, w: usize| {
        let l = width(s);
        format!(" {s}{}", " ".repeat(w - l - 1))
    };
    let line = |l: &str, m: &str, r: &str| {
        let mut s = String::from(l);
        for (i, w) in widths.iter().enumerate() {
            s.push_str(&"─".repeat(*w));
            s.push_str(if i + 1 < widths.len() { m } else { r });
        }
        s
    };
    let mut out = vec![line("┌", "┬", "┐")];
    let mut hl = String::from("│");
    for (i, h) in header.iter().enumerate() {
        hl.push_str(&center(h, widths[i]));
        hl.push('│');
    }
    out.push(hl);
    out.push(line("├", "┼", "┤"));
    for r in &table {
        let mut l = String::from("│");
        for (i, c) in r.iter().enumerate() {
            l.push_str(&center(c, widths[i]));
            l.push('│');
        }
        out.push(l);
    }
    out.push(line("└", "┴", "┘"));
    write_out(vm, &out.join("\n"), false);
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- process

fn process_exit(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let code = if a.arg(0).is_nullish() {
        vm.exit_code
    } else {
        let n = vm.to_number(&a.arg(0))?;
        if n.fract() != 0.0 || !n.is_finite() {
            let d = vm.inspect_default(&a.arg(0))?;
            let e = vm.make_error(
                ErrKind::RangeError,
                &format!(
                    "The value of \"code\" is out of range. It must be an integer. Received {d}"
                ),
            );
            e.set_prop("code", Value::str("ERR_OUT_OF_RANGE"), ALL);
            return Err(Ctl::Throw(Value::Obj(e)));
        }
        n as i32
    };
    vm.exit_code = code;
    Err(Ctl::Exit(code))
}

fn exit_code_get(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(if vm.exit_code_set {
        Value::Num(vm.exit_code as f64)
    } else {
        Value::Undefined
    })
}

fn exit_code_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = a.arg(0);
    if v.is_nullish() {
        vm.exit_code = 0;
        vm.exit_code_set = false;
    } else {
        vm.exit_code = vm.to_i32(&v)?;
        vm.exit_code_set = true;
    }
    Ok(Value::Undefined)
}

fn process_cwd(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::string(vm.host.cwd()))
}

fn process_chdir(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = str_arg(vm, a, 0)?;
    if let Err(e) = vm.host.chdir(&p) {
        let cwd = vm.host.cwd();
        let err = crate::fs::fs_error(vm, e.kind, "chdir", &cwd, Some(&p));
        return Err(Ctl::Throw(err));
    }
    Ok(Value::Undefined)
}

fn process_hrtime(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let ns = vm.perf_now() * 1e6 + 1_000_000_000_000.0;
    let mut s = (ns / 1e9).floor();
    let mut n = ns - s * 1e9;
    if let Value::Obj(prev) = a.arg(0) {
        let pv = Value::Obj(prev);
        let ps = vm.get_index(&pv, 0)?;
        let pn = vm.get_index(&pv, 1)?;
        let ps = vm.to_number(&ps)?;
        let pn = vm.to_number(&pn)?;
        s -= ps;
        n -= pn;
        if n < 0.0 {
            s -= 1.0;
            n += 1e9;
        }
    }
    Ok(vm.arr(vec![Value::Num(s), Value::Num(n.floor())]))
}

fn process_hrtime_bigint(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let ns = vm.perf_now() * 1e6 + 1_000_000_000_000.0;
    Ok(Value::BigInt(Rc::new(crate::bigint::BigInt::from_f64(
        ns.floor(),
    ))))
}

fn process_uptime(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(0.05 + vm.perf_now() / 1000.0))
}

fn process_memory_usage(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let o = crate::builtins::new_obj_from(
        vm,
        vec![
            ("rss", Value::Num(45_000_000.0)),
            ("heapTotal", Value::Num(6_000_000.0)),
            ("heapUsed", Value::Num(4_500_000.0)),
            ("external", Value::Num(1_500_000.0)),
            ("arrayBuffers", Value::Num(10_000.0)),
        ],
    );
    Ok(Value::Obj(o))
}

fn process_next_tick(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = a.arg(0);
    if !f.is_callable() {
        let d = vm.inspect_default(&f)?;
        let e = vm.make_error(
            ErrKind::TypeError,
            &format!(
                "The \"callback\" argument must be of type function. Received {}",
                received(&f, &d)
            ),
        );
        e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    }
    let rest = a.args.iter().skip(1).cloned().collect();
    vm.ticks.push_back((f, rest));
    Ok(Value::Undefined)
}

pub fn received(v: &Value, inspected: &str) -> String {
    match v {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Obj(o) if o.is_callable() => {
            let n = Vm::func_name(o);
            format!("function {n}")
        }
        Value::Obj(_) => format!(
            "an instance of {}",
            inspected.split_whitespace().next().unwrap_or("Object")
        ),
        Value::Str(_) => format!("type string ({inspected})"),
        other => format!("type {} ({inspected})", other.type_of()),
    }
}

fn stream_write(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let fd = match slots(a).first() {
        Some(Value::Num(n)) => *n as i32,
        _ => 1,
    };
    let v = a.arg(0);
    let s = match &v {
        Value::Str(s) => s.to_string(),
        Value::Obj(o) if matches!(o.borrow().kind, Kind::TypedArray { .. }) => {
            String::from_utf8_lossy(&vm.typed_bytes(o).unwrap_or_default()).into_owned()
        }
        other => {
            let d = vm.inspect_default(other)?;
            let e = vm.make_error(
                ErrKind::TypeError,
                &format!("The \"chunk\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received {}", received(other, &d)),
            );
            e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
            return Err(Ctl::Throw(Value::Obj(e)));
        }
    };
    if fd == 2 {
        vm.stderr.push_str(&s);
    } else {
        vm.stdout.push_str(&s);
    }
    // Callback argument (last function) runs asynchronously.
    if let Some(cb) = a.args.iter().skip(1).find(|x| x.is_callable()) {
        vm.ticks.push_back((cb.clone(), vec![]));
    }
    Ok(Value::Bool(true))
}

fn read_stdin(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    stdin_rest(vm)
}

/// Everything left on standard input. At a terminal that means everything up to
/// an end-of-file that may not have been typed yet, so the run may suspend.
pub fn stdin_rest(vm: &mut Vm) -> JsResult<Value> {
    if vm.interactive {
        if !vm.stdin_eof {
            return Err(vm.need_input());
        }
        let stdin = vm.stdin.clone().unwrap_or_default();
        let rest = stdin[vm.stdin_pos.min(stdin.len())..].to_string();
        vm.stdin_pos = stdin.len();
        return Ok(Value::string(rest));
    }
    if vm.stdin_consumed {
        return Ok(Value::str(""));
    }
    vm.stdin_consumed = true;
    Ok(Value::string(vm.stdin.clone().unwrap_or_default()))
}

/// One typed line (with its newline), `null` at end-of-file. At a terminal a
/// line nobody has typed yet suspends the run.
fn read_line(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let stdin = vm.stdin.clone().unwrap_or_default();
    if vm.stdin_pos >= stdin.len() {
        if vm.interactive && !vm.stdin_eof {
            return Err(vm.need_input());
        }
        return Ok(Value::Null);
    }
    let rest = &stdin[vm.stdin_pos..];
    let end = rest.find('\n').map(|p| p + 1).unwrap_or(rest.len());
    let line = rest[..end].to_string();
    vm.stdin_pos += end;
    Ok(Value::string(line))
}

fn emit_warning(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let w = a.arg(0);
    let (name, msg) = match &w {
        Value::Obj(_) => {
            let n = vm.get_str(&w, "name")?;
            let m = vm.get_str(&w, "message")?;
            (vm.to_str(&n)?, vm.to_str(&m)?)
        }
        v => {
            let t = a.arg(1);
            let name = match &t {
                Value::Str(s) => s.to_string(),
                Value::Obj(_) => {
                    let ty = vm.get_str(&t, "type")?;
                    if ty.is_undefined() {
                        "Warning".to_string()
                    } else {
                        vm.to_str(&ty)?
                    }
                }
                _ => "Warning".to_string(),
            };
            (name, vm.to_str(v)?)
        }
    };
    let pid = vm.host.pid();
    vm.stderr.push_str(&format!("(node:{pid}) {name}: {msg}\n(Use `node --trace-warnings ...` to show where the warning was created)\n"));
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- timers

fn timer_delay(vm: &mut Vm, v: &Value) -> JsResult<f64> {
    let d = if v.is_undefined() {
        1.0
    } else {
        vm.to_number(v)?
    };
    Ok(if !(1.0..=2147483647.0).contains(&d) || d.is_nan() {
        1.0
    } else {
        d.trunc()
    })
}

fn add_timer(vm: &mut Vm, a: &Args, repeat: bool, immediate: bool) -> JsResult<Value> {
    let cb = a.arg(0);
    if !cb.is_callable() {
        let d = vm.inspect_default(&cb)?;
        let e = vm.make_error(
            ErrKind::TypeError,
            &format!(
                "The \"callback\" argument must be of type function. Received {}",
                received(&cb, &d)
            ),
        );
        e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    }
    let (delay, rest) = if immediate {
        (0.0, a.args.iter().skip(1).cloned().collect())
    } else {
        (
            timer_delay(vm, &a.arg(1))?,
            a.args.iter().skip(2).cloned().collect(),
        )
    };
    let id = vm.timer_id;
    vm.timer_id += 1;
    let proto = vm.timeout_proto(immediate);
    let obj = vm.obj_with(Some(proto), Kind::Internal(vec![Value::Num(id as f64)]));
    if !immediate {
        obj.set_prop("_idleTimeout", Value::Num(delay), ALL);
        obj.set_prop("_onTimeout", cb.clone(), ALL);
        obj.set_prop(
            "_repeat",
            if repeat {
                Value::Num(delay)
            } else {
                Value::Null
            },
            ALL,
        );
    }
    vm.timer_seq += 1;
    // `Environment::GetNow` updates the loop clock before a timer starts, so
    // the due time counts from now, not from the turn's cached time.
    let when = vm.clock() + delay;
    vm.timers.push(Timer {
        id,
        when,
        seq: vm.timer_seq,
        callback: cb,
        args: rest,
        interval: if repeat { Some(delay) } else { None },
        obj: obj.clone(),
        immediate,
        io: false,
        dur: delay,
    });
    Ok(Value::Obj(obj))
}

fn set_timeout(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    add_timer(vm, a, false, false)
}
fn set_interval(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    add_timer(vm, a, true, false)
}
fn set_immediate(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    add_timer(vm, a, false, true)
}

fn timer_id_of(v: &Value) -> Option<u64> {
    match v {
        Value::Obj(o) => match &o.borrow().kind {
            Kind::Internal(s) => match s.first() {
                Some(Value::Num(n)) => Some(*n as u64),
                _ => None,
            },
            _ => None,
        },
        Value::Num(n) => Some(*n as u64),
        Value::Str(s) => s.parse().ok(),
        _ => None,
    }
}

fn clear_timer(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Some(id) = timer_id_of(&a.arg(0)) {
        vm.timers.retain(|t| t.id != id);
    }
    Ok(Value::Undefined)
}

fn timer_ref(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        o.set_hidden("%unref", Value::Bool(false));
    }
    Ok(a.this.clone())
}

fn timer_unref(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        o.set_hidden("%unref", Value::Bool(true));
    }
    Ok(a.this.clone())
}

fn timer_has_ref(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        return Ok(Value::Bool(!matches!(
            o.own_value("%unref"),
            Some(Value::Bool(true))
        )));
    }
    Ok(Value::Bool(false))
}

fn timer_refresh(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Some(id) = timer_id_of(&a.this) {
        let now = vm.clock();
        vm.timer_seq += 1;
        let seq = vm.timer_seq;
        for t in vm.timers.iter_mut() {
            if t.id == id {
                t.when = now
                    + t.interval
                        .unwrap_or_else(|| match t.obj.own_value("_idleTimeout") {
                            Some(Value::Num(n)) => n,
                            _ => 1.0,
                        });
                t.seq = seq;
            }
        }
    }
    Ok(a.this.clone())
}

fn timer_close(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Some(id) = timer_id_of(&a.this) {
        vm.timers.retain(|t| t.id != id);
    }
    Ok(a.this.clone())
}

fn timer_to_primitive(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(timer_id_of(&a.this)
        .map(|i| Value::Num(i as f64))
        .unwrap_or(Value::Num(f64::NAN)))
}

impl<'h> Vm<'h> {
    pub fn perf_now(&mut self) -> f64 {
        // Monotonic virtual milliseconds since start (plus a boot offset).
        30.0 + self.clock()
    }

    fn timeout_proto(&mut self, immediate: bool) -> Obj {
        let key = if immediate {
            "%ImmediateProto"
        } else {
            "%TimeoutProto"
        };
        if let Some(Value::Obj(p)) = self.global.own_value(key) {
            return p;
        }
        let p = self.new_object();
        let ctor = self.native_fn(
            if immediate { "Immediate" } else { "Timeout" },
            0,
            |vm, _a| Err(vm.type_error("Illegal constructor")),
        );
        ctor.set_prop("prototype", Value::Obj(p.clone()), 0);
        p.set_hidden("constructor", Value::Obj(ctor));
        self.method(&p, "ref", 0, timer_ref);
        self.method(&p, "unref", 0, timer_unref);
        self.method(&p, "hasRef", 0, timer_has_ref);
        self.method(&p, "refresh", 0, timer_refresh);
        self.method(&p, "close", 0, timer_close);
        let tp = self.syms.to_primitive.clone();
        self.method_sym(&p, &tp, "[Symbol.toPrimitive]", 0, timer_to_primitive);
        self.global.set_hidden(key, Value::Obj(p.clone()));
        p
    }
}

// ---------------------------------------------------------------- modules

fn is_relative(spec: &str) -> bool {
    spec.starts_with("./")
        || spec.starts_with("../")
        || spec.starts_with('/')
        || spec == "."
        || spec == ".."
}

pub fn join_path(base: &str, rel: &str) -> String {
    let full = if rel.starts_with('/') {
        rel.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), rel)
    };
    normalize(&full)
}

pub fn normalize(p: &str) -> String {
    let abs = p.starts_with('/');
    let mut parts: Vec<&str> = vec![];
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().map(|x| *x != "..").unwrap_or(false) {
                    parts.pop();
                } else if !abs {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let j = parts.join("/");
    if abs {
        format!("/{j}")
    } else if j.is_empty() {
        ".".into()
    } else {
        j
    }
}

pub fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".into(),
        Some(i) => p[..i].to_string(),
        None => ".".into(),
    }
}

impl<'h> Vm<'h> {
    fn is_file(&mut self, p: &str) -> bool {
        matches!(self.host.stat(p), Ok(st) if !st.is_dir)
    }
    fn is_dir(&mut self, p: &str) -> bool {
        matches!(self.host.stat(p), Ok(st) if st.is_dir)
    }

    fn resolve_file(&mut self, p: &str) -> Option<String> {
        if self.is_file(p) {
            return Some(p.to_string());
        }
        for ext in [".js", ".json", ".cjs", ".mjs"] {
            let c = format!("{p}{ext}");
            if self.is_file(&c) {
                return Some(c);
            }
        }
        if self.is_dir(p) {
            let pj = format!("{p}/package.json");
            if let Ok(b) = self.host.read_file(&pj) {
                let txt = String::from_utf8_lossy(&b).into_owned();
                if let Ok(v) = crate::builtins::json::parse_json(self, &txt) {
                    if let Ok(Value::Str(m)) = self.get_str(&v, "main") {
                        let mp = join_path(p, &m);
                        if let Some(f) = self.resolve_file(&mp) {
                            return Some(f);
                        }
                    }
                }
            }
            for idx in ["index.js", "index.json", "index.cjs", "index.mjs"] {
                let c = format!("{p}/{idx}");
                if self.is_file(&c) {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Resolves a specifier to a builtin name or an absolute file path.
    pub fn resolve_module(&mut self, spec: &str, dir: &str) -> Option<String> {
        let bare = spec.strip_prefix("node:").unwrap_or(spec);
        // Internal modules are visible to the built-in modules only.
        if bare.starts_with("internal/")
            && dir.starts_with("/node_internal")
            && js_module_source(bare).is_some()
        {
            return Some(format!("node:{bare}"));
        }
        if spec.starts_with("node:") || BUILTINS.contains(&bare) {
            if BUILTINS.contains(&bare) {
                return Some(format!("node:{bare}"));
            }
            return None;
        }
        if let Some(p) = spec.strip_prefix("file://") {
            return self.resolve_file(p);
        }
        if is_relative(spec) {
            let p = join_path(dir, spec);
            return self.resolve_file(&p);
        }
        // node_modules lookup.
        let mut d = dir.to_string();
        loop {
            let cand = format!("{}/node_modules/{spec}", d.trim_end_matches('/'));
            if let Some(f) = self.resolve_file(&cand) {
                return Some(f);
            }
            if d == "/" || d.is_empty() {
                break;
            }
            d = dirname(&d);
        }
        None
    }

    fn module_not_found(&mut self, spec: &str, parents: &[String]) -> Ctl {
        let mut msg = format!("Cannot find module '{spec}'");
        if !parents.is_empty() {
            msg.push_str("\nRequire stack:");
            for p in parents {
                msg.push_str(&format!("\n- {p}"));
            }
        }
        let e = self.make_error(ErrKind::Error, &msg);
        let frames: Vec<String> = [
            "Module._resolveFilename (node:internal/modules/cjs/loader:1564:15)",
            "wrapResolveFilename (node:internal/modules/cjs/loader:1118:27)",
            "defaultResolveImplForCJSLoading (node:internal/modules/cjs/loader:1142:10)",
            "resolveForCJSWithHooks (node:internal/modules/cjs/loader:1169:12)",
            "Module._load (node:internal/modules/cjs/loader:1341:5)",
            "wrapModuleLoad (node:internal/modules/cjs/loader:261:19)",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (user, _) = self.stack_frames(true);
        // Replace the require native frame lines with resolution frames.
        let mut all = frames;
        let skip = user
            .iter()
            .position(|f| f.starts_with("Module.require"))
            .unwrap_or(0);
        all.extend(user.into_iter().skip(skip));
        self.append_tail(&mut all);
        all.truncate(self.stack_limit);
        if let Kind::Error(ed) = &mut e.borrow_mut().kind {
            ed.frames = all;
            ed.arrow = Some("node:internal/modules/cjs/loader:1568\n  throw err;\n  ^\n".into());
        }
        e.set_prop("code", Value::str("MODULE_NOT_FOUND"), ALL);
        let rs: Vec<Value> = parents.iter().map(|p| Value::string(p.clone())).collect();
        let arr = self.arr(rs);
        e.set_prop("requireStack", arr, ALL);
        Ctl::Throw(Value::Obj(e))
    }

    /// The object behind `require.cache`, shared by every module.
    fn require_cache(&mut self) -> Obj {
        if let Some(Value::Obj(o)) = self.global.own_value("%requireCache") {
            return o;
        }
        let o = self.new_object();
        self.global
            .set_hidden("%requireCache", Value::Obj(o.clone()));
        o
    }

    fn cached_module(&self, key: &str) -> Option<Value> {
        self.modules
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    /// require(spec) from a module in `dir`.
    pub fn require(&mut self, spec: &str, dir: &str, parent: &str) -> JsResult<Value> {
        let Some(key) = self.resolve_module(spec, dir) else {
            let mut parents = vec![];
            if !parent.is_empty() {
                parents.push(parent.to_string());
            }
            return Err(self.module_not_found(spec, &parents));
        };
        if let Some(m) = self.cached_module(&key) {
            // Deleting a `require.cache` entry makes the next require reload.
            let evicted = !key.starts_with("node:")
                && self.require_cache().own_value(&key).is_none()
                && key != self.main_file;
            if !evicted {
                return self.get_str(&m, "exports");
            }
            self.modules.retain(|(k, _)| *k != key);
        }
        if let Some(name) = key.strip_prefix("node:") {
            let exports = self.builtin_module(name)?;
            let m = self.new_object();
            m.set_prop("exports", exports.clone(), ALL);
            m.set_prop("loaded", Value::Bool(true), ALL);
            self.modules.push((key, Value::Obj(m)));
            return Ok(exports);
        }
        // The requiring module becomes `module.parent` and lists the child.
        let parent_mod = self.cached_module(parent);
        self.loading_parent = Some(parent_mod.clone().unwrap_or(Value::Null));
        let m = self.load_file_module(&key)?;
        if let Some(pm) = parent_mod {
            let ch = self.get_str(&pm, "children")?;
            if let Value::Obj(a) = &ch {
                if let Kind::Array(v) = &mut a.borrow_mut().kind {
                    v.push(m.clone());
                }
            }
        }
        self.get_str(&m, "exports")
    }

    fn make_module_object(&mut self, path: &str) -> Obj {
        let m = self.new_object();
        m.set_prop("id", Value::string(path.to_string()), ALL);
        m.set_prop("path", Value::string(dirname(path)), ALL);
        let exports = self.new_object();
        m.set_prop("exports", Value::Obj(exports), ALL);
        m.set_prop("filename", Value::string(path.to_string()), ALL);
        m.set_prop("loaded", Value::Bool(false), ALL);
        let ch = self.arr(vec![]);
        m.set_prop("children", ch, ALL);
        let paths = self.arr(vec![]);
        m.set_prop("paths", paths, ALL);
        let parent = self.loading_parent.take().unwrap_or(Value::Null);
        m.set_hidden("parent", parent);
        m
    }

    pub fn make_require(&mut self, path: &str) -> Obj {
        let f = self.native_fn_slots(
            "require",
            1,
            require_fn,
            vec![
                Value::string(dirname(path)),
                Value::string(path.to_string()),
            ],
        );
        let resolve = self.native_fn_slots(
            "resolve",
            1,
            require_resolve,
            vec![Value::string(dirname(path))],
        );
        f.set_prop("resolve", Value::Obj(resolve), ALL);
        let cache = self.require_cache();
        f.set_prop("cache", Value::Obj(cache), ALL);
        if let Some(Value::Obj(m)) = self.cached_module(&self.main_file.clone()) {
            f.set_prop("main", Value::Obj(m), ALL);
        }
        let frames = self.frames_sym();
        f.set_sym(
            &frames,
            Value::str(
                "Module._compile (node:internal/modules/cjs/loader:1929:14)\nObject..js (node:internal/modules/cjs/loader:2060:10)\nModule.load (node:internal/modules/cjs/loader:1651:32)\nModule._load (node:internal/modules/cjs/loader:1443:12)\nwrapModuleLoad (node:internal/modules/cjs/loader:261:19)\nModule.require (node:internal/modules/cjs/loader:1674:12)\nrequire (node:internal/modules/helpers:157:16)",
            ),
            0,
        );
        f
    }

    pub fn frames_sym(&mut self) -> Rc<Symbol> {
        if let Some((_, s)) = self.symbol_registry.iter().find(|(k, _)| k == "%frames") {
            return s.clone();
        }
        let s = Rc::new(Symbol {
            desc: Some(JsStr::new("%frames")),
            private: true,
            registered: false,
        });
        self.symbol_registry.push(("%frames".into(), s.clone()));
        s
    }

    /// Compiles source; returns the code and whether it is an ES module.
    pub fn compile_source(
        &mut self,
        src: &str,
        file: &str,
        force_module: Option<bool>,
        cjs_params: &[&str],
    ) -> JsResult<(Rc<crate::bytecode::Code>, bool)> {
        let pk = self.prof_enter(|| format!("[parse+compile] {file}"));
        let t0 = self.prof.as_ref().map(|_| crate::profile::now_ns());
        let force = match force_module {
            None => "auto",
            Some(true) => "module",
            Some(false) => "script",
        };
        let params = cjs_params.join(",");
        let parts: [&[u8]; 5] = [
            b"program",
            force.as_bytes(),
            params.as_bytes(),
            file.as_bytes(),
            src.as_bytes(),
        ];
        if let Some((k, code, is_module)) = crate::codecache::get(&parts) {
            let code = self.first_use(k, code);
            let fname: Rc<str> = if is_module {
                Rc::from(format!("file://{file}").as_str())
            } else {
                Rc::from(file)
            };
            self.register_source(fname, Rc::from(src));
            self.note_program(src, file, force_module, &params, &code);
            self.prof_source(file, src.len(), t0, t0, true);
            self.prof_leave(pk);
            return Ok((code, is_module));
        }
        let r = self.compile_source_inner(src, file, force_module, cjs_params);
        if let Ok((code, is_module)) = &r {
            let k = crate::codecache::put(&parts, code.clone(), *is_module);
            self.cache_seen.insert(k);
            self.note_program(src, file, force_module, &params, code);
        }
        self.prof_leave(pk);
        r
    }

    fn note_program(
        &mut self,
        src: &str,
        file: &str,
        force_module: Option<bool>,
        params: &str,
        code: &Rc<crate::bytecode::Code>,
    ) {
        let kind = crate::snapshot::UnitKind::Program {
            force_module,
            params: params.to_owned(),
            file: file.to_owned(),
        };
        self.note_unit(kind, src, code);
    }

    /// Compiles a unit a heap snapshot names, as the compile that made it
    /// did: from the cache when it is there (shared the first time this VM
    /// asks for it, a fresh copy after that, as `first_use`), else afresh.
    pub(crate) fn restore_unit(
        &mut self,
        kind: &crate::snapshot::UnitKind,
        src: &str,
        seen: &mut std::collections::HashSet<crate::codecache::CacheKey>,
    ) -> Result<Rc<crate::bytecode::Code>, String> {
        use crate::snapshot::UnitKind;
        let (head, force, params, file): (&[u8], &str, &str, &str) = match kind {
            UnitKind::Program {
                force_module,
                params,
                file,
            } => (
                b"program",
                match force_module {
                    None => "auto",
                    Some(true) => "module",
                    Some(false) => "script",
                },
                params,
                file,
            ),
            UnitKind::Eval { global, file } => {
                (b"eval", if *global { "global" } else { "local" }, "", file)
            }
        };
        let program: [&[u8]; 5] = [
            head,
            force.as_bytes(),
            params.as_bytes(),
            file.as_bytes(),
            src.as_bytes(),
        ];
        let eval: [&[u8]; 4] = [head, force.as_bytes(), file.as_bytes(), src.as_bytes()];
        let parts: &[&[u8]] = match kind {
            UnitKind::Program { .. } => &program,
            UnitKind::Eval { .. } => &eval,
        };
        if let Some((k, code, _)) = crate::codecache::get(parts) {
            return Ok(if seen.insert(k) {
                code
            } else {
                code.fresh_copy()
            });
        }
        let failed = |_: Ctl| format!("{file} no longer compiles");
        match kind {
            UnitKind::Program { force_module, .. } => {
                let ps: Vec<&str> = if params.is_empty() {
                    Vec::new()
                } else {
                    params.split(',').collect()
                };
                let (code, is_module) = self
                    .compile_source_inner(src, file, *force_module, &ps)
                    .map_err(failed)?;
                seen.insert(crate::codecache::put(parts, code.clone(), is_module));
                Ok(code)
            }
            UnitKind::Eval { global, .. } => {
                let code = self
                    .compile_eval_source(src, file, *global, None, 0)
                    .map_err(failed)?;
                seen.insert(crate::codecache::put(parts, code.clone(), false));
                Ok(code)
            }
        }
    }

    fn compile_source_inner(
        &mut self,
        src: &str,
        file: &str,
        force_module: Option<bool>,
        cjs_params: &[&str],
    ) -> JsResult<(Rc<crate::bytecode::Code>, bool)> {
        let t0 = self.prof.as_ref().map(|_| crate::profile::now_ns());
        let chars: Vec<char> = src.chars().collect();
        let try_module = force_module.unwrap_or(false);
        let parsed = crate::parser::parse_chars(&chars, try_module);
        let (prog, is_module) = match parsed {
            Ok((prog, saw)) => {
                if saw && !try_module && force_module.is_none() {
                    match crate::parser::parse_chars(&chars, true) {
                        Ok((p, _)) => (p, true),
                        Err(e) => return Err(self.syntax_error_from(e, src, file, true)),
                    }
                } else {
                    (prog, try_module)
                }
            }
            Err(e) => {
                // Module syntax errors in script goal: retry as a module.
                if force_module.is_none() && e.msg.contains("import") {
                    match crate::parser::parse_chars(&chars, true) {
                        Ok((p, _)) => (p, true),
                        Err(e2) => return Err(self.syntax_error_from(e2, src, file, false)),
                    }
                } else {
                    return Err(self.syntax_error_from(e, src, file, try_module));
                }
            }
        };
        let fname: Rc<str> = if is_module {
            Rc::from(format!("file://{file}").as_str())
        } else {
            Rc::from(file)
        };
        let text: Rc<str> = Rc::from(src);
        self.register_source(fname.clone(), text.clone());
        let t1 = self.prof.as_ref().map(|_| crate::profile::now_ns());
        let mut c = crate::compiler::Compiler::new(fname.clone(), &chars, is_module);
        c.set_text(text);
        c.completion = file == "[eval]" || file == "[stdin]";
        let params: Vec<&str> = if is_module {
            vec!["%ns", "%import", "%meta"]
        } else {
            cjs_params.to_vec()
        };
        let r = c.compile_program(&prog, &params, is_module);
        self.prof_source(file, src.len(), t0, t1, false);
        match r {
            Ok(code) => Ok((code, is_module)),
            Err(e) => Err(self.syntax_error_from(e, src, &fname, is_module)),
        }
    }

    pub fn syntax_error_from(
        &mut self,
        e: crate::lexer::SyntaxErr,
        src: &str,
        file: &str,
        esm: bool,
    ) -> Ctl {
        let err = self.make_error(ErrKind::SyntaxError, &e.msg);
        let line_text = src
            .split('\n')
            .nth(e.line.saturating_sub(1) as usize)
            .unwrap_or("")
            .trim_end_matches('\r');
        let caret = format!(
            "{}{}",
            " ".repeat(e.col.saturating_sub(1) as usize),
            "^".repeat(e.len as usize)
        );
        let shown = if esm && !file.starts_with("file://") {
            format!("file://{file}")
        } else {
            file.to_string()
        };
        let arrow = format!("{shown}:{}\n{line_text}\n{caret}\n", e.line);
        let mut frames: Vec<String> = if esm {
            vec![
                "compileSourceTextModule (node:internal/modules/esm/utils:346:16)".into(),
                "ModuleLoader.moduleStrategy (node:internal/modules/esm/translators:107:18)".into(),
                "#translate (node:internal/modules/esm/loader:540:20)".into(),
                "afterLoad (node:internal/modules/esm/loader:596:29)".into(),
                "ModuleLoader.loadAndTranslate (node:internal/modules/esm/loader:601:12)".into(),
                "#createModuleJob (node:internal/modules/esm/loader:624:36)".into(),
                "#getJobFromResolveResult (node:internal/modules/esm/loader:343:34)".into(),
                "ModuleLoader.getModuleJobForImport (node:internal/modules/esm/loader:311:41)"
                    .into(),
                "async onImport.tracePromise.__proto__ (node:internal/modules/esm/loader:664:25)"
                    .into(),
            ]
        } else if self.tail == Tail::Eval {
            vec![
                "makeContextifyScript (node:internal/vm:185:14)".into(),
                "compileScript (node:internal/process/execution:386:10)".into(),
            ]
        } else if self.tail == Tail::Check {
            vec![
                "wrapSafe (node:internal/modules/cjs/loader:1861:18)".into(),
                "checkSyntax (node:internal/main/check_syntax:88:3)".into(),
            ]
        } else {
            vec![
                "wrapSafe (node:internal/modules/cjs/loader:1861:18)".into(),
                "Module._compile (node:internal/modules/cjs/loader:1903:20)".into(),
            ]
        };
        if !esm && self.tail != Tail::Eval && self.tail != Tail::Check {
            let (user, _) = self.stack_frames(true);
            // Loader frames between the compile and the requiring code.
            frames.push("Object..js (node:internal/modules/cjs/loader:2060:10)".into());
            frames.push("Module.load (node:internal/modules/cjs/loader:1651:32)".into());
            frames.push("Module._load (node:internal/modules/cjs/loader:1443:12)".into());
            frames.push("wrapModuleLoad (node:internal/modules/cjs/loader:261:19)".into());
            let skip = user
                .iter()
                .position(|f| f.starts_with("Module.require"))
                .unwrap_or(user.len());
            frames.extend(user.into_iter().skip(skip));
            if self.frames.is_empty() {
                frames.push("Module.executeUserEntryPoint [as runMain] (node:internal/modules/run_main:154:5)".into());
                frames.push("node:internal/main/run_main_module:33:47".into());
            } else {
                self.append_tail(&mut frames);
            }
        }
        frames.truncate(self.stack_limit);
        if let Kind::Error(ed) = &mut err.borrow_mut().kind {
            ed.frames = frames;
            ed.arrow = Some(arrow);
        }
        Ctl::Throw(Value::Obj(err))
    }

    /// Loads a module; one that fails to load leaves no cache entry.
    pub fn load_file_module(&mut self, path: &str) -> JsResult<Value> {
        let r = self.load_file_module_inner(path);
        if r.is_err() {
            if let Some(i) = self.modules.iter().rposition(|(k, _)| k == path) {
                self.modules.remove(i);
            }
            let cache = self.require_cache();
            cache.borrow_mut().props.remove(&Key::str(path));
        }
        r
    }

    fn load_file_module_inner(&mut self, path: &str) -> JsResult<Value> {
        let bytes = match self.host.read_file(path) {
            Ok(b) => b,
            Err(_) => return Err(self.module_not_found(path, &[])),
        };
        self.charge_module_load(bytes.len());
        let src = String::from_utf8_lossy(&bytes).into_owned();
        let m = self.make_module_object(path);
        let mv = Value::Obj(m.clone());
        self.modules.push((path.to_string(), mv.clone()));
        self.require_cache().set_prop(path, mv.clone(), ALL);
        if path.ends_with(".json") {
            let v =
                match crate::builtins::json::parse_json(self, src.trim_start_matches('\u{feff}')) {
                    Ok(v) => v,
                    Err(Ctl::Throw(e)) => {
                        if let Value::Obj(eo) = &e {
                            let m = self.get_str(&e, "message")?;
                            let ms = self.to_str(&m)?;
                            eo.set_hidden("message", Value::string(format!("{path}: {ms}")));
                        }
                        return Err(Ctl::Throw(e));
                    }
                    Err(o) => return Err(o),
                };
            m.set_prop("exports", v, ALL);
            m.set_prop("loaded", Value::Bool(true), ALL);
            return Ok(mv);
        }
        let force = if path.ends_with(".mjs") {
            Some(true)
        } else if path.ends_with(".cjs") {
            Some(false)
        } else {
            None
        };
        let src_body = src.strip_prefix('\u{feff}').unwrap_or(&src);
        let (code, is_module) = self.compile_source(
            src_body,
            path,
            force,
            &["exports", "require", "module", "__filename", "__dirname"],
        )?;
        let caps: Rc<[CellRef]> = Rc::from(Vec::new());
        let f = self.make_closure(code, caps);
        if is_module {
            let ns = self.obj_with(None, Kind::Ordinary);
            ns.borrow_mut().tag = Some("Module");
            let tag = self.syms.to_string_tag.clone();
            ns.set_sym(&tag, Value::str("Module"), 0);
            m.set_prop("exports", Value::Obj(ns.clone()), ALL);
            m.set_hidden("%esm", Value::Bool(true));
            let imp = self.native_fn_slots(
                "import",
                1,
                esm_import_fn,
                vec![
                    Value::string(dirname(path)),
                    Value::string(path.to_string()),
                ],
            );
            let meta = self.new_object();
            meta.set_prop("url", Value::string(format!("file://{path}")), ALL);
            meta.set_prop("filename", Value::string(path.to_string()), ALL);
            meta.set_prop("dirname", Value::string(dirname(path)), ALL);
            let saved_meta = self.import_meta.replace(meta.clone());
            if path == self.main_file {
                self.tail = Tail::Esm;
                self.is_esm_main = true;
            }
            let r = self.call(
                &Value::Obj(f),
                Value::Undefined,
                vec![Value::Obj(ns), Value::Obj(imp), Value::Obj(meta)],
            );
            self.import_meta = saved_meta;
            let p = r?;
            // Top-level await: settle the module's promise.
            if let Value::Obj(po) = &p {
                if matches!(po.borrow().kind, Kind::Promise(_)) {
                    self.esm_promises.push(po.clone());
                    let _ = self.settled_value(&p)?;
                }
            }
        } else {
            let exports = self.get_str(&mv, "exports")?;
            let req = self.make_require(path);
            self.call(
                &Value::Obj(f),
                exports.clone(),
                vec![
                    exports,
                    Value::Obj(req),
                    mv.clone(),
                    Value::string(path.to_string()),
                    Value::string(dirname(path)),
                ],
            )?;
        }
        m.set_prop("loaded", Value::Bool(true), ALL);
        Ok(mv)
    }

    /// ESM import: namespace object for a specifier.
    pub fn import_namespace(&mut self, spec: &str, dir: &str, parent: &str) -> JsResult<Value> {
        let Some(key) = self.resolve_module(spec, dir) else {
            let e = self.make_error(
                ErrKind::Error,
                &format!(
                    "Cannot find module '{}' imported from {parent}",
                    join_path(dir, spec)
                ),
            );
            e.set_prop("code", Value::str("ERR_MODULE_NOT_FOUND"), ALL);
            let u = self.arr(vec![]);
            let _ = u;
            e.set_prop(
                "url",
                Value::string(format!("file://{}", join_path(dir, spec))),
                ALL,
            );
            return Err(Ctl::Throw(Value::Obj(e)));
        };
        let m = if let Some(m) = self.cached_module(&key) {
            m
        } else if key.starts_with("node:") {
            self.require(spec, dir, parent)?;
            self.cached_module(&key).unwrap()
        } else {
            self.load_file_module(&key)?
        };
        let exports = self.get_str(&m, "exports")?;
        let is_esm = matches!(&m, Value::Obj(mo) if mo.own_value("%esm").is_some());
        if is_esm {
            return Ok(exports);
        }
        // CommonJS / builtin: default + named exports.
        let ns = self.obj_with(None, Kind::Ordinary);
        let tag = self.syms.to_string_tag.clone();
        ns.set_sym(&tag, Value::str("Module"), 0);
        if let Value::Obj(eo) = &exports {
            // Built-in modules export their (non-enumerable) methods too.
            let keys: Vec<JsStr> = if key.starts_with("node:") {
                self.own_keys(eo)?
                    .into_iter()
                    .filter_map(|k| match k {
                        Key::Str(s) => Some(s),
                        _ => None,
                    })
                    .collect()
            } else {
                self.own_enum_keys(eo)?
            };
            for k in keys {
                if k.as_str() == "default" {
                    continue;
                }
                let v = self.get(&exports, &Key::Str(k.clone()))?;
                ns.set_prop(&k, v, ENUMERABLE);
            }
        }
        ns.set_prop("default", exports, ENUMERABLE);
        Ok(Value::Obj(ns))
    }

    pub fn dynamic_import(&mut self, spec: &Value) -> JsResult<Value> {
        let s = self.to_str(spec)?;
        let (dir, parent) = match self.frames.last() {
            Some(f) => {
                let file = f.code.file.trim_start_matches("file://").to_string();
                (dirname(&file), file)
            }
            None => (self.host.cwd(), String::new()),
        };
        let p = self.new_promise();
        match self.import_namespace(&s, &dir, &parent) {
            Ok(ns) => self.resolve_promise(&p, ns)?,
            Err(Ctl::Throw(e)) => self.reject_promise(&p, e),
            Err(o) => return Err(o),
        }
        Ok(Value::Obj(p))
    }

    /// Records a source's parse and compile times while profiling (`t0` before
    /// parsing, `t1` before compiling).
    fn prof_source(
        &mut self,
        file: &str,
        bytes: usize,
        t0: Option<u64>,
        t1: Option<u64>,
        cached: bool,
    ) {
        if let (Some(p), Some(t0), Some(t1)) = (self.prof.as_deref_mut(), t0, t1) {
            let t2 = crate::profile::now_ns();
            p.source(crate::profile::SourceStat {
                file: file.to_string(),
                bytes,
                parse_ns: t1.saturating_sub(t0),
                compile_ns: t2.saturating_sub(t1),
                cached,
            });
        }
    }

    /// Runs source in the global scope (indirect eval / new Function).
    pub fn eval_source(&mut self, src: &str, file: &str, completion: bool) -> JsResult<Value> {
        self.eval_source_with(src, file, completion, false)
    }

    /// `eval_source` with `global_scope`: top-level declarations become global
    /// properties shared with later scripts (a browser's classic scripts).
    pub fn eval_source_with(
        &mut self,
        src: &str,
        file: &str,
        completion: bool,
        global_scope: bool,
    ) -> JsResult<Value> {
        let _ = completion;
        let pk = self.prof_enter(|| format!("[parse+compile] {file}"));
        let t0 = self.prof.as_ref().map(|_| crate::profile::now_ns());
        let parts: [&[u8]; 4] = [
            b"eval",
            if global_scope { b"global" } else { b"local" },
            file.as_bytes(),
            src.as_bytes(),
        ];
        let code = match crate::codecache::get(&parts) {
            Some((k, code, _)) => {
                let code = self.first_use(k, code);
                self.register_source(Rc::from(file), Rc::from(src));
                self.prof_source(file, src.len(), t0, t0, true);
                self.prof_leave(pk);
                code
            }
            None => {
                let code = self.compile_eval_source(src, file, global_scope, t0, pk)?;
                let k = crate::codecache::put(&parts, code.clone(), false);
                self.cache_seen.insert(k);
                code
            }
        };
        let kind = crate::snapshot::UnitKind::Eval {
            global: global_scope,
            file: file.to_owned(),
        };
        self.note_unit(kind, src, &code);
        let caps: Rc<[CellRef]> = Rc::from(Vec::new());
        let f = self.make_closure(code, caps);
        self.call(&Value::Obj(f), Value::Obj(self.global.clone()), vec![])
    }

    /// Cached code for this realm: shared the first time the realm compiles
    /// the source, a fresh copy after that (as recompiling would give).
    fn first_use(
        &mut self,
        k: crate::codecache::CacheKey,
        code: Rc<crate::bytecode::Code>,
    ) -> Rc<crate::bytecode::Code> {
        if self.cache_seen.insert(k) {
            code
        } else {
            code.fresh_copy()
        }
    }

    fn compile_eval_source(
        &mut self,
        src: &str,
        file: &str,
        global_scope: bool,
        t0: Option<u64>,
        pk: usize,
    ) -> JsResult<Rc<crate::bytecode::Code>> {
        let chars: Vec<char> = src.chars().collect();
        let prog = match crate::parser::parse_chars(&chars, false) {
            Ok((p, _)) => p,
            Err(e) => {
                self.prof_leave(pk);
                let err = self.make_error(ErrKind::SyntaxError, &e.msg);
                return Err(Ctl::Throw(Value::Obj(err)));
            }
        };
        let fname: Rc<str> = Rc::from(file);
        let text: Rc<str> = Rc::from(src);
        self.register_source(fname.clone(), text.clone());
        let t1 = self.prof.as_ref().map(|_| crate::profile::now_ns());
        let mut c = crate::compiler::Compiler::new(fname, &chars, false);
        c.set_text(text);
        c.global_scope = global_scope;
        let compiled = c.compile_eval(&prog);
        self.prof_source(file, src.len(), t0, t1, false);
        self.prof_leave(pk);
        match compiled {
            Ok(code) => Ok(code),
            Err(e) => {
                let err = self.make_error(ErrKind::SyntaxError, &e.msg);
                Err(Ctl::Throw(Value::Obj(err)))
            }
        }
    }

    fn builtin_module(&mut self, name: &str) -> JsResult<Value> {
        match name {
            "fs" => return Ok(crate::fs::make_fs(self)),
            "fs/promises" => {
                let fs = crate::fs::make_fs(self);
                return self.get_str(&fs, "promises");
            }
            "path" | "path/posix" => return Ok(crate::nodelib::make_path(self)),
            "os" => return Ok(crate::nodelib::make_os(self)),
            "crypto" => return Ok(crate::nodelib::make_crypto(self)),
            "process" => {
                return Ok(self
                    .process
                    .clone()
                    .map(Value::Obj)
                    .unwrap_or(Value::Undefined))
            }
            "buffer" => {
                return self.load_internal_js("node:buffer", BUFFER_MODULE, "buffer");
            }
            "timers" => {
                let g = Value::Obj(self.global.clone());
                let o = self.new_object();
                for n in [
                    "setTimeout",
                    "setInterval",
                    "setImmediate",
                    "clearTimeout",
                    "clearInterval",
                    "clearImmediate",
                ] {
                    let v = self.get_str(&g, n)?;
                    o.set_prop(n, v, ALL);
                }
                return Ok(Value::Obj(o));
            }
            "readline/promises" => {
                let r = self.require("readline", "/", "")?;
                return self.get_str(&r, "promises");
            }
            "assert/strict" => {
                let r = self.require("assert", "/", "")?;
                return self.get_str(&r, "strict");
            }
            "util/types" => {
                let u = self.require("util", "/", "")?;
                return self.get_str(&u, "types");
            }
            "perf_hooks" => {
                let p = self.get_str(&Value::Obj(self.global.clone()), "performance")?;
                let o = self.new_object();
                o.set_prop("performance", p, ALL);
                return Ok(Value::Obj(o));
            }
            _ => {}
        }
        if let Some(src) = js_module_source(name) {
            return self.load_internal_js(&format!("node:{name}"), src, name);
        }
        // Unsupported built-ins: an empty module object with a warning-free stub.
        let o = self.new_object();
        Ok(Value::Obj(o))
    }

    /// Runs an internal JS module with (exports, require, module, binding).
    pub fn load_internal_js(&mut self, file: &str, src: &str, name: &str) -> JsResult<Value> {
        let (code, _) = self.compile_source(
            src,
            file,
            Some(false),
            &["exports", "require", "module", "binding"],
        )?;
        let caps: Rc<[CellRef]> = Rc::from(Vec::new());
        let f = self.make_closure(code, caps);
        let m = self.new_object();
        let exports = self.new_object();
        m.set_prop("exports", Value::Obj(exports.clone()), ALL);
        let req = self.make_require("/node_internal/x.js");
        let binding = self.binding();
        let saved = self.modules.len();
        let _ = saved;
        let _ = name;
        self.call(
            &Value::Obj(f),
            Value::Obj(exports.clone()),
            vec![
                Value::Obj(exports),
                Value::Obj(req),
                Value::Obj(m.clone()),
                binding,
            ],
        )?;
        self.get_str(&Value::Obj(m), "exports")
    }

    fn binding(&mut self) -> Value {
        if let Some(Value::Obj(b)) = self.global.own_value("%binding") {
            return Value::Obj(b);
        }
        let b = crate::nodelib::make_binding(self);
        self.global.set_hidden("%binding", Value::Obj(b.clone()));
        Value::Obj(b)
    }

    // ------------------------------------------------------------ event loop

    /// Runs microtasks, timers and immediates until nothing is pending,
    /// phase by phase like libuv: due timers, I/O completions, immediates.
    pub fn event_loop(&mut self) -> JsResult<()> {
        self.event_loop_until(&mut |_| false, f64::INFINITY)
            .map(|_| ())
    }

    /// The event loop, stopping early: once `done` holds at a turn with nothing
    /// left to run at the current time (the clock is not moved on for work
    /// further off), or before the clock would pass `deadline_ms`. Returns
    /// whether `done` held. What is still scheduled stays scheduled.
    pub fn event_loop_until(
        &mut self,
        done: &mut dyn FnMut(&mut Vm) -> bool,
        deadline_ms: f64,
    ) -> JsResult<bool> {
        loop {
            self.drain_after(None)?;
            // Between tasks: a safe point to reclaim cyclic garbage.
            crate::gc::maybe_collect();
            // Each turn starts by reading the clock (`uv__update_time`).
            let now = self.clock();
            let mut ran = false;
            // Timers phase: due timers by time then creation order; Node
            // runs ticks and promise jobs between two of them.
            let mut prev: Option<f64> = None;
            while let Some(i) = self.next_due_timer(now) {
                if let Some(d) = prev {
                    let b = if d == self.timers[i].dur {
                        Batch::List
                    } else {
                        Batch::Lists
                    };
                    self.drain_after(Some(b))?;
                }
                prev = Some(self.timers[i].dur);
                self.fire_timer(i)?;
                ran = true;
            }
            if ran {
                self.drain_after(None)?;
            }
            // Poll phase: completed I/O, one callback at a time, in the order the
            // completions arrived (network replies land at their simulated time).
            let poll_now = self.clock();
            while let Some(i) = self
                .timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.io && t.when <= poll_now)
                .min_by(|a, b| {
                    a.1.when
                        .partial_cmp(&b.1.when)
                        .unwrap()
                        .then(a.1.seq.cmp(&b.1.seq))
                })
                .map(|(i, _)| i)
            {
                self.fire_timer(i)?;
                self.drain_after(None)?;
                ran = true;
            }
            // Check phase: the immediates queued before it started.
            let ids: Vec<u64> = self
                .timers
                .iter()
                .filter(|t| t.immediate && !t.io)
                .map(|t| t.id)
                .collect();
            let mut first = true;
            for id in ids {
                let Some(i) = self.timers.iter().position(|t| t.id == id) else {
                    continue;
                };
                if !first {
                    self.drain_after(Some(Batch::Immediates))?;
                }
                first = false;
                self.fire_timer(i)?;
                ran = true;
            }
            // Workers: once this context can do no more, the others take their
            // turn, and what they send back arrives here.
            if self.workers.is_some() {
                // What a worker has written reaches the terminal through its
                // parent, which passes it on when it next comes round.
                let mut moved = self.flush_worker_output();
                moved |= self.worker_events()?;
                moved |= self.deliver_inbox()?;
                moved |= self.flush_ports()?;
                moved |= self.poll_async_waits()?;
                if !ran && !moved {
                    moved |= self.run_workers()?;
                    moved |= self.flush_worker_output();
                    moved |= self.worker_events()?;
                    moved |= self.deliver_inbox()?;
                    moved |= self.flush_ports()?;
                    moved |= self.poll_async_waits()?;
                }
                if moved {
                    self.drain_after(None)?;
                    ran = true;
                }
            }
            if ran {
                continue;
            }
            if done(self) {
                return Ok(true);
            }
            let mut next = self
                .timers
                .iter()
                .filter(|t| !matches!(t.obj.own_value("%unref"), Some(Value::Bool(true))))
                .map(|t| t.when)
                .min_by(|a, b| a.partial_cmp(b).unwrap());
            if self.workers.is_some() {
                // A worker's timer keeps the whole program awake.
                for w in self.worker_timer_times() {
                    next = Some(match next {
                        Some(n) => n.min(w),
                        None => w,
                    });
                }
            }
            match next {
                Some(w) if w > deadline_ms => return Ok(false),
                Some(w) => self.elapsed_ms = self.clock().max(w),
                None => {
                    // Nothing anywhere can run: a worker left waiting for a
                    // message that will never come ends here.
                    let main = self.workers.is_some() && self.workers_current() == 0;
                    if main && self.end_idle_workers() {
                        continue;
                    }
                    break;
                }
            }
        }
        Ok(done(self))
    }

    pub fn next_due_timer(&self, now: f64) -> Option<usize> {
        self.timers
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.immediate && t.when <= now)
            .min_by(|a, b| {
                a.1.when
                    .partial_cmp(&b.1.when)
                    .unwrap()
                    .then(a.1.seq.cmp(&b.1.seq))
            })
            .map(|(i, _)| i)
    }

    /// Drains ticks and promise jobs after a callback, in the context Node
    /// would (between two callbacks of a batch, or after the batch).
    pub fn drain_after(&mut self, between: Option<Batch>) -> JsResult<()> {
        let saved = self.drain;
        self.drain = Drain {
            between,
            tick: self.stdout.len() + self.stderr.len() != self.out_mark,
        };
        let r = self.run_microtasks_checked();
        self.drain = saved;
        self.out_mark = self.stdout.len() + self.stderr.len();
        r
    }

    pub fn fire_timer(&mut self, i: usize) -> JsResult<()> {
        let t = &self.timers[i];
        let (cb, args, obj, immediate, io) = (
            t.callback.clone(),
            t.args.clone(),
            t.obj.clone(),
            t.immediate,
            t.io,
        );
        match t.interval {
            Some(d) => {
                self.timer_seq += 1;
                let seq = self.timer_seq;
                let now = self.clock();
                let t = &mut self.timers[i];
                t.when = now + d;
                t.seq = seq;
            }
            None => {
                self.timers.remove(i);
            }
        }
        self.out_mark = self.stdout.len() + self.stderr.len();
        let (tail, frame, this) = if io {
            (Tail::None, 0, Value::Undefined)
        } else if immediate {
            (Tail::Immediate, IMMEDIATE_FRAME, Value::Obj(obj))
        } else {
            (Tail::Timer, TIMEOUT_FRAME, Value::Obj(obj))
        };
        self.tail = tail;
        self.timer_frame = frame;
        let r = self.call(&cb, this, args);
        self.timer_frame = 0;
        r?;
        self.tail = Tail::Microtask(None, false);
        Ok(())
    }

    pub fn run_microtasks_checked(&mut self) -> JsResult<()> {
        self.run_microtasks()?;
        // Unhandled rejections are fatal (Node's default `throw` mode).
        while !self.pending_rejections.is_empty() {
            let p = self.pending_rejections.remove(0);
            let (st, val) = self.promise_state(&p).unwrap();
            let handled = matches!(&p.borrow().kind, Kind::Promise(pd) if pd.handled);
            if st != PromiseState::Rejected || handled {
                continue;
            }
            // process.on('unhandledRejection') listeners take over.
            if self.emit_process_event(
                "unhandledRejection",
                vec![val.clone(), Value::Obj(p.clone())],
            )? {
                self.run_microtasks()?;
                continue;
            }
            return Err(Ctl::Throw(self.rejection_error(val)));
        }
        Ok(())
    }

    fn rejection_error(&mut self, val: Value) -> Value {
        let is_err = matches!(&val, Value::Obj(o) if matches!(o.borrow().kind, Kind::Error(_)));
        if is_err {
            mark_async(&val);
            return val;
        }
        let d = match &val {
            Value::Str(s) => s.to_string(),
            v => self.inspect_default(v).unwrap_or_default(),
        };
        let e = self.make_error(
            ErrKind::Error,
            &format!(
                "This error originated either by throwing inside of an async function without a catch block, or by rejecting a promise which was not handled with .catch(). The promise rejected with the reason \"{d}\"."
            ),
        );
        e.set_hidden("name", Value::str("UnhandledPromiseRejection"));
        e.set_prop("code", Value::str("ERR_UNHANDLED_REJECTION"), ALL);
        if let Kind::Error(ed) = &mut e.borrow_mut().kind {
            ed.frames = vec![
                "throwUnhandledRejectionsMode (node:internal/process/promises:392:7)".into(),
                "processPromiseRejections (node:internal/process/promises:475:17)".into(),
                "process.processTicksAndRejections (node:internal/process/task_queues:106:37)"
                    .into(),
            ];
            ed.arrow = Some("node:internal/process/promises:392\n      new UnhandledPromiseRejection(reason);\n      ^\n".into());
        }
        Value::Obj(e)
    }

    /// Emits an event on `process` (EventEmitter). Returns whether any
    /// listener existed.
    pub fn emit_process_event(&mut self, name: &str, args: Vec<Value>) -> JsResult<bool> {
        let Some(p) = self.process.clone() else {
            return Ok(false);
        };
        let pv = Value::Obj(p);
        let lc = self.get_str(&pv, "listenerCount")?;
        if !lc.is_callable() {
            return Ok(false);
        }
        let n = self.call(&lc, pv.clone(), vec![Value::str(name)])?;
        if self.to_number(&n)? == 0.0 {
            return Ok(false);
        }
        let emit = self.get_str(&pv, "emit")?;
        let mut a = vec![Value::str(name)];
        a.extend(args);
        self.call(&emit, pv, a)?;
        Ok(true)
    }

    // ------------------------------------------------------------ reporting

    fn arrow_for_site(&self, site: &Site) -> Option<String> {
        let src = self.source_for(&site.file)?;
        let line = src
            .split('\n')
            .nth(site.line.saturating_sub(1) as usize)?
            .trim_end_matches('\r');
        let caret = format!("{}^", " ".repeat(site.col.saturating_sub(1) as usize));
        Some(format!("{}:{}\n{line}\n{caret}\n", site.file, site.line))
    }

    /// Writes Node's report for an uncaught exception to stderr.
    pub fn report_uncaught(&mut self, v: &Value) {
        let (is_error, arrow, from_promise) = match v {
            Value::Obj(o) => match &o.borrow().kind {
                Kind::Error(ed) => (
                    true,
                    ed.arrow.clone(),
                    if ed.from_async { ed.site.clone() } else { None },
                ),
                _ => (false, None, None),
            },
            _ => (false, None, None),
        };
        let arrow = match arrow {
            Some(a) => Some(a),
            None => {
                let site = if is_error && from_promise.is_some() {
                    from_promise
                } else {
                    self.throw_site.clone()
                };
                site.and_then(|s| self.arrow_for_site(&s))
            }
        };
        let body = match v {
            Value::Str(s) => s.to_string(),
            _ => {
                let o = crate::inspect::Opts {
                    custom_inspect: false,
                    depth: Some(5.0),
                    ..Default::default()
                };
                match self.inspect(v, &o) {
                    Ok(s) => s,
                    Err(_) => "Uncaught exception".into(),
                }
            }
        };
        let mut out = String::new();
        if is_error {
            if let Some(a) = arrow {
                out.push_str(&a);
                out.push('\n');
            }
            out.push_str(&body);
            out.push('\n');
        } else {
            out.push('\n');
            if let Some(a) = arrow {
                out.push_str(&a);
            }
            out.push_str(&body);
            out.push('\n');
            if !matches!(v, Value::Obj(_)) {
                out.push_str(
                    "(Use `node --trace-uncaught ...` to show where the exception was thrown)\n",
                );
            }
        }
        out.push_str(&format!("\nNode.js {NODE_VERSION}\n"));
        self.stderr.push_str(&out);
    }
}

/// Errors escaping microtasks report their construction site.
pub fn mark_async(v: &Value) {
    if let Value::Obj(o) = v {
        if let Kind::Error(ed) = &mut o.borrow_mut().kind {
            ed.from_async = true;
        }
    }
}

fn require_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let dir = match &s[0] {
        Value::Str(d) => d.to_string(),
        _ => "/".into(),
    };
    let parent = match &s[1] {
        Value::Str(d) => d.to_string(),
        _ => String::new(),
    };
    let spec = a.arg(0);
    let Value::Str(spec) = spec else {
        let d = vm.inspect_default(&spec)?;
        let e = vm.make_error(
            ErrKind::TypeError,
            &format!(
                "The \"id\" argument must be of type string. Received {}",
                received(&spec, &d)
            ),
        );
        e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    };
    if spec.is_empty() {
        let e = vm.make_error(
            ErrKind::TypeError,
            "The argument 'id' must be a non-empty string. Received ''",
        );
        e.set_prop("code", Value::str("ERR_INVALID_ARG_VALUE"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    }
    let parent = if parent.starts_with("/node_internal") {
        String::new()
    } else {
        parent
    };
    vm.require(&spec, &dir, &parent)
}

fn require_resolve(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let dir = match &s[0] {
        Value::Str(d) => d.to_string(),
        _ => "/".into(),
    };
    let spec = str_arg(vm, a, 0)?;
    match vm.resolve_module(&spec, &dir) {
        Some(k) => Ok(Value::string(
            k.strip_prefix("node:").map(|x| x.to_string()).unwrap_or(k),
        )),
        None => Err(vm.module_not_found(&spec, &[])),
    }
}

fn esm_import_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let dir = match &s[0] {
        Value::Str(d) => d.to_string(),
        _ => "/".into(),
    };
    let parent = match &s[1] {
        Value::Str(d) => format!("file://{}", d.as_str()),
        _ => String::new(),
    };
    let spec = str_arg(vm, a, 0)?;
    vm.import_namespace(&spec, &dir, &parent)
}

fn global_getter(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Obj(vm.global.clone()))
}

fn noop(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Undefined)
}

fn fetch(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let p = vm.new_promise();
    let e = vm.make_error(ErrKind::TypeError, "fetch failed");
    let cause = vm.make_error(ErrKind::Error, "getaddrinfo ENOTFOUND");
    cause.set_prop("code", Value::str("ENOTFOUND"), ALL);
    e.set_hidden("cause", Value::Obj(cause));
    vm.reject_promise(&p, Value::Obj(e));
    Ok(Value::Obj(p))
}

fn performance_now(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(vm.perf_now()))
}

fn performance_time_origin(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(vm.start_micros as f64 / 1000.0))
}

pub fn install(vm: &mut Vm) {
    let g = vm.global.clone();
    // console
    let c = vm.new_object();
    let log = vm.method(&c, "log", 0, console_log);
    c.set_hidden("info", Value::Obj(log.clone()));
    c.set_hidden("debug", Value::Obj(log));
    let err = vm.method(&c, "error", 0, console_error);
    c.set_hidden("warn", Value::Obj(err));
    vm.method(&c, "dir", 0, console_dir);
    vm.method(&c, "dirxml", 0, console_log);
    vm.method(&c, "assert", 0, console_assert);
    vm.method(&c, "trace", 0, console_trace);
    vm.method(&c, "count", 0, console_count);
    vm.method(&c, "countReset", 0, console_count_reset);
    vm.method(&c, "time", 0, console_time);
    vm.method(&c, "timeEnd", 0, console_time_end);
    vm.method(&c, "timeLog", 0, console_time_log);
    vm.method(&c, "group", 0, console_group);
    vm.method(&c, "groupCollapsed", 0, console_group);
    vm.method(&c, "groupEnd", 0, console_group_end);
    vm.method(&c, "table", 1, console_table);
    vm.method(&c, "clear", 0, noop);
    vm.set_global("console", Value::Obj(c));
    // timers
    for (n, l, f) in [
        ("setTimeout", 2, set_timeout as NativeFn),
        ("setInterval", 2, set_interval),
        ("setImmediate", 1, set_immediate),
        ("clearTimeout", 1, clear_timer),
        ("clearInterval", 1, clear_timer),
        ("clearImmediate", 1, clear_timer),
        ("queueMicrotask", 1, crate::promise::queue_microtask),
        ("fetch", 1, fetch),
    ] {
        vm.method(&g, n, l, f);
    }
    let gg = vm.native_fn("get global", 0, global_getter);
    g.borrow_mut().props.insert(
        Key::str("global"),
        Prop {
            slot: Slot::Accessor(Some(gg), None),
            flags: CONFIGURABLE,
        },
    );
    // performance
    let perf = vm.new_object();
    vm.method(&perf, "now", 0, performance_now);
    // A getter (as Node's is, on the prototype): the heap does not hold the
    // clock's origin, so a heap image of a booted VM serves any start time.
    vm.getter(&perf, "timeOrigin", performance_time_origin);
    vm.method(&perf, "mark", 1, noop);
    vm.method(&perf, "measure", 1, noop);
    vm.set_global("performance", Value::Obj(perf));
    // process (native core; bootstrap.js makes it an EventEmitter)
    let p = vm.new_object();
    let tag = vm.syms.to_string_tag.clone();
    p.set_sym(&tag, Value::str("process"), CONFIGURABLE);
    let argv: Vec<Value> = vm.argv.iter().map(|s| Value::string(s.clone())).collect();
    let av = vm.arr(argv);
    p.set_prop("argv", av, ALL);
    let ea = vm.arr(vec![]);
    p.set_prop("execArgv", ea, ALL);
    let env = vm.new_object();
    for (k, v) in vm.env.clone() {
        env.set_prop(&k, Value::string(v), ALL);
    }
    p.set_prop("env", Value::Obj(env), ALL);
    p.set_prop("platform", Value::str("linux"), ALL);
    p.set_prop("arch", Value::str("x64"), ALL);
    p.set_prop("version", Value::str(NODE_VERSION), ALL);
    let versions = crate::builtins::new_obj_from(
        vm,
        vec![
            ("node", Value::str(&NODE_VERSION[1..])),
            ("v8", Value::str("13.6.233.17-node.35")),
            ("uv", Value::str("1.51.0")),
            ("modules", Value::str("137")),
        ],
    );
    p.set_prop("versions", Value::Obj(versions), ALL);
    let pid = vm.host.pid();
    p.set_prop("pid", Value::Num(pid as f64), ALL);
    p.set_prop("ppid", Value::Num((pid.saturating_sub(1)) as f64), ALL);
    p.set_prop("title", Value::str("node"), ALL);
    p.set_prop("execPath", Value::str("/usr/bin/node"), ALL);
    let release = crate::builtins::new_obj_from(
        vm,
        vec![("name", Value::str("node")), ("lts", Value::str("Krypton"))],
    );
    p.set_prop("release", Value::Obj(release), ALL);
    let exit = vm.method(&p, "exit", 1, process_exit);
    let _ = exit;
    let g2 = vm.native_fn("get exitCode", 0, exit_code_get);
    let s2 = vm.native_fn("set exitCode", 1, exit_code_set);
    p.borrow_mut().props.insert(
        Key::str("exitCode"),
        Prop {
            slot: Slot::Accessor(Some(g2), Some(s2)),
            flags: ENUMERABLE | CONFIGURABLE,
        },
    );
    vm.method(&p, "cwd", 0, process_cwd);
    vm.method(&p, "chdir", 1, process_chdir);
    let hr = vm.method(&p, "hrtime", 1, process_hrtime);
    vm.method(&hr, "bigint", 0, process_hrtime_bigint);
    vm.method(&p, "uptime", 0, process_uptime);
    vm.method(&p, "memoryUsage", 0, process_memory_usage);
    vm.method(&p, "nextTick", 1, process_next_tick);
    vm.method(&p, "emitWarning", 1, emit_warning);
    vm.method(&p, "umask", 0, |_vm, _a| Ok(Value::Num(18.0)));
    vm.method(&p, "getuid", 0, |_vm, _a| Ok(Value::Num(1000.0)));
    vm.method(&p, "getgid", 0, |_vm, _a| Ok(Value::Num(1000.0)));
    vm.method(&p, "cpuUsage", 0, |vm, _a| {
        let o = crate::builtins::new_obj_from(
            vm,
            vec![
                ("user", Value::Num(20000.0)),
                ("system", Value::Num(5000.0)),
            ],
        );
        Ok(Value::Obj(o))
    });
    for (name, fd) in [("stdout", 1.0), ("stderr", 2.0)] {
        let s = vm.new_object();
        let w = vm.native_fn_slots("write", 1, stream_write, vec![Value::Num(fd)]);
        s.set_prop("write", Value::Obj(w), ALL);
        s.set_prop("isTTY", Value::Bool(false), ALL);
        s.set_prop("fd", Value::Num(fd), ALL);
        s.set_prop("columns", Value::Num(80.0), ALL);
        s.set_prop("rows", Value::Num(24.0), ALL);
        s.set_prop("writable", Value::Bool(true), ALL);
        vm.method(&s, "end", 0, noop);
        vm.method(&s, "on", 2, |_vm, a| Ok(a.this.clone()));
        vm.method(&s, "once", 2, |_vm, a| Ok(a.this.clone()));
        vm.method(&s, "cork", 0, noop);
        vm.method(&s, "uncork", 0, noop);
        vm.method(&s, "getWindowSize", 0, |vm, _a| {
            Ok(vm.arr(vec![Value::Num(80.0), Value::Num(24.0)]))
        });
        vm.method(&s, "hasColors", 0, |_vm, _a| Ok(Value::Bool(false)));
        p.set_prop(name, Value::Obj(s), ALL);
    }
    vm.method(&p, "%readStdin", 0, read_stdin);
    vm.method(&p, "%readLine", 0, read_line);
    vm.method(&p, "%isInteractive", 0, |vm, _a| {
        Ok(Value::Bool(vm.interactive))
    });
    vm.process = Some(p.clone());
    vm.set_global("process", Value::Obj(p));
    // util.inspect is reachable for custom inspectors.
    let insp = vm.native_fn("inspect", 2, crate::nodelib::util_inspect);
    g.set_hidden("%inspect", Value::Obj(insp));
    // Bootstrap in JS: EventEmitter-based process, Buffer, URL, ...
    let src = include_str!("../js/bootstrap.js");
    if let Err(e) = vm.load_internal_js("node:internal/bootstrap", src, "bootstrap") {
        let msg = match e {
            Ctl::Throw(v) => vm.inspect_default(&v).unwrap_or_default(),
            _ => "fatal".into(),
        };
        vm.stderr.push_str(&format!("bootstrap failed: {msg}\n"));
    }
}
