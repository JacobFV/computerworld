//! `sys`, the `_os` primitives under `os`, `time`, `hashlib`, `platform`, `secrets`.
use super::{new_module, set_fn, set_val};
use crate::builtins::*;
use crate::io::os_error;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

pub const VERSION: &str = "3.12.3 (main, Mar 23 2026, 19:04:32) [GCC 13.3.0]";

pub fn exec_snippet(vm: &mut Vm, m: &Rc<Module>, src: &str) {
    let name = m.name.to_string();
    match crate::compile_source(vm, src, &format!("<frozen {name}>"), "exec") {
        Ok(code) => {
            let frame = vm.new_frame(code, m.dict.clone(), None);
            if vm.charge_depth().is_ok() {
                if let Err(e) = vm.execute(frame, None) {
                    let v = vm.exc_value(e);
                    let s = vm.repr(&v).unwrap_or_default();
                    vm.write_stderr(&format!("internal error initializing {name}: {s}\n"));
                }
            }
        }
        Err(e) => {
            let v = vm.exc_value(e);
            let s = vm.repr(&v).unwrap_or_default();
            vm.write_stderr(&format!("internal error compiling {name}: {s}\n"));
        }
    }
}

pub fn make_sys(vm: &mut Vm) -> Value {
    let m = new_module("sys");
    let argv: Vec<Value> = vm.argv.iter().map(|a| Value::str(a)).collect();
    set_val(&m, "argv", Value::list(argv.clone()));
    set_val(&m, "orig_argv", Value::list(argv));
    set_val(&m, "stdin", crate::io::std_file(0));
    set_val(&m, "stdout", crate::io::std_file(1));
    set_val(&m, "stderr", crate::io::std_file(2));
    for n in ["stdin", "stdout", "stderr"] {
        let v = m.dict.borrow().get_str(n).unwrap();
        set_val(&m, &format!("__{n}__"), v);
    }
    set_val(&m, "path", Value::list(vec![Value::str(&vm.script_dir)]));
    set_val(&m, "modules", Value::Dict(vm.modules.clone()));
    set_val(&m, "version", Value::str(VERSION));
    set_val(&m, "hexversion", Value::Int(0x030c03f0));
    set_val(&m, "platform", Value::str("linux"));
    set_val(&m, "byteorder", Value::str("little"));
    set_val(&m, "maxsize", Value::Int(i64::MAX));
    set_val(&m, "maxunicode", Value::Int(0x10ffff));
    set_val(&m, "executable", Value::str("/usr/bin/python3"));
    set_val(&m, "prefix", Value::str("/usr"));
    set_val(&m, "base_prefix", Value::str("/usr"));
    set_val(&m, "exec_prefix", Value::str("/usr"));
    set_val(&m, "dont_write_bytecode", Value::Bool(true));
    set_val(&m, "api_version", Value::Int(1013));
    set_val(
        &m,
        "copyright",
        Value::str("Copyright (c) 2001-2024 Python Software Foundation."),
    );
    set_val(
        &m,
        "builtin_module_names",
        Value::tuple(
            [
                "builtins",
                "sys",
                "time",
                "math",
                "_random",
                "json",
                "re",
                "gc",
                "_collections",
            ]
            .iter()
            .map(|s| Value::str(s))
            .collect(),
        ),
    );
    set_fn(&m, "exit", sys_exit);
    set_fn(&m, "getrecursionlimit", |vm, _| {
        Ok(Value::Int(vm.recursion_limit as i64))
    });
    set_fn(&m, "setrecursionlimit", |vm, a| {
        let n = to_int_arg(vm, a.args.first().unwrap_or(&Value::Int(1000)))?;
        if n < 1 {
            return Err(value_err("recursion limit must be greater or equal than 1"));
        }
        // Bounded so the simulator's own stack is never at risk.
        vm.recursion_limit = (n as usize).min(10_000);
        Ok(Value::None)
    });
    set_fn(&m, "setswitchinterval", |vm, a| {
        let s = crate::bfuncs::float_from(vm, a.args.first().unwrap_or(&Value::Float(0.005)))?;
        if s <= 0.0 {
            return Err(value_err("switch interval must be strictly positive"));
        }
        // The quantum scales with the interval, so a program can ask for finer
        // or coarser interleaving just as CPython's does.
        vm.start_scheduler();
        if let Some(sc) = vm.sched.as_mut() {
            sc.set_switch_interval(s);
        }
        Ok(Value::None)
    });
    set_fn(&m, "getswitchinterval", |vm, _| {
        Ok(Value::Float(
            vm.sched
                .as_ref()
                .map(|s| s.switch_interval)
                .unwrap_or(crate::sched::DEFAULT_SWITCH_INTERVAL),
        ))
    });
    set_fn(&m, "get_int_max_str_digits", |vm, _| {
        Ok(Value::Int(vm.int_max_str_digits as i64))
    });
    set_fn(&m, "set_int_max_str_digits", |vm, a| {
        let n = to_int_arg(vm, a.args.first().unwrap_or(&Value::Int(4300)))?;
        if n != 0 && n < 640 {
            return Err(value_err("maxdigits must be 0 or larger than 640"));
        }
        vm.int_max_str_digits = n as usize;
        Ok(Value::None)
    });
    set_fn(&m, "exc_info", |vm, _| {
        Ok(match vm.exc_stack.last().cloned() {
            Some(e) => {
                let t = Value::Class(vm.type_of(&e));
                Value::tuple(vec![t, e, Value::None])
            }
            None => Value::tuple(vec![Value::None, Value::None, Value::None]),
        })
    });
    set_fn(&m, "exception", |vm, _| {
        Ok(vm.exc_stack.last().cloned().unwrap_or(Value::None))
    });
    set_fn(&m, "getsizeof", |vm, a| {
        let v = a.args.first().cloned().unwrap_or(Value::None);
        let n = match &v {
            Value::Int(_) => 28,
            Value::Bool(_) => 28,
            Value::Float(_) => 24,
            Value::Str(s) => 49 + s.s.len() as i64,
            Value::List(l) => 56 + 8 * l.borrow().len() as i64,
            Value::Tuple(t) => 40 + 8 * t.len() as i64,
            Value::Dict(d) => 64 + 24 * d.borrow().len() as i64,
            Value::None => 16,
            _ => {
                let _ = vm;
                48
            }
        };
        Ok(Value::Int(n))
    });
    set_fn(&m, "intern", |_, a| {
        Ok(a.args.first().cloned().unwrap_or(Value::None))
    });
    set_fn(&m, "getdefaultencoding", |_, _| Ok(Value::str("utf-8")));
    set_fn(&m, "getfilesystemencoding", |_, _| Ok(Value::str("utf-8")));
    set_fn(&m, "_getframe", |_, _| {
        Err(value_err("call stack is not deep enough"))
    });
    set_fn(&m, "settrace", |_, _| Ok(Value::None));
    set_fn(&m, "gettrace", |_, _| Ok(Value::None));
    set_fn(&m, "_format_exception", |vm, a| {
        let exc = a.args.first().cloned().unwrap_or(Value::None);
        let chain = match a.args.get(1) {
            Some(v) => vm.truthy(v)?,
            None => true,
        };
        Ok(Value::string(crate::format_exception(
            vm,
            &exc,
            if chain { 0 } else { 20 },
        )))
    });
    set_fn(&m, "_stack", |vm, _| {
        let items = vm
            .stack_summary()
            .into_iter()
            .map(|(f, l, n)| {
                Value::tuple(vec![Value::str(&f), Value::Int(l as i64), Value::str(&n)])
            })
            .collect();
        Ok(Value::list(items))
    });
    set_fn(&m, "_source_line", |vm, a| {
        let file = match a.args.first() {
            Some(Value::Str(s)) => s.s.clone(),
            _ => return Ok(Value::None),
        };
        let line = to_int_arg(vm, a.args.get(1).unwrap_or(&Value::Int(0)))?;
        let text = vm.sources.get(&file).and_then(|src| {
            src.lines()
                .nth((line - 1).max(0) as usize)
                .map(|l| l.to_string())
        });
        Ok(text.map(Value::string).unwrap_or(Value::None))
    });
    exec_snippet(
        vm,
        &m,
        r#"
class _StructSeq(tuple):
    _fields = ()
    _name = ''
    def __repr__(self):
        return self._name + '(' + ', '.join(f + '=' + repr(v) for f, v in zip(self._fields, self)) + ')'
    def __getattr__(self, name):
        try:
            return self[self._fields.index(name)]
        except ValueError:
            raise AttributeError(name) from None
class _VersionInfo(_StructSeq):
    _fields = ('major', 'minor', 'micro', 'releaselevel', 'serial')
    _name = 'sys.version_info'
version_info = _VersionInfo((3, 12, 3, 'final', 0))
class _FloatInfo(_StructSeq):
    _fields = ('max', 'max_exp', 'max_10_exp', 'min', 'min_exp', 'min_10_exp', 'dig', 'mant_dig', 'epsilon', 'radix', 'rounds')
    _name = 'sys.float_info'
float_info = _FloatInfo((1.7976931348623157e+308, 1024, 308, 2.2250738585072014e-308, -1021, -307, 15, 53, 2.220446049250313e-16, 2, 1))
class _IntInfo(_StructSeq):
    _fields = ('bits_per_digit', 'sizeof_digit', 'default_max_str_digits', 'str_digits_check_threshold')
    _name = 'sys.int_info'
int_info = _IntInfo((30, 4, 4300, 640))
class _Namespace:
    def __init__(self, **kw):
        self.__dict__.update(kw)
    def __repr__(self):
        return 'namespace(' + ', '.join(k + '=' + repr(v) for k, v in self.__dict__.items()) + ')'
implementation = _Namespace(name='cpython', cache_tag='cpython-312', version=version_info, hexversion=51119088, _multiarch='x86_64-linux-gnu')
class _Flags(_StructSeq):
    _fields = ('debug', 'inspect', 'interactive', 'optimize', 'dont_write_bytecode', 'no_user_site', 'no_site', 'ignore_environment', 'verbose', 'bytes_warning', 'quiet', 'hash_randomization', 'isolated', 'dev_mode', 'utf8_mode', 'warn_default_encoding', 'safe_path', 'int_max_str_digits')
    _name = 'sys.flags'
flags = _Flags((0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, False, 0, 0, False, -1))
del _Flags, _IntInfo, _FloatInfo, _VersionInfo
"#,
    );
    Value::Module(m)
}

fn sys_exit(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Err(err_args("SystemExit", a.args))
}

fn path_arg(vm: &mut Vm, v: Option<&Value>) -> PyResult<String> {
    match v {
        None => Ok(".".into()),
        Some(Value::Str(s)) => Ok(s.s.clone()),
        Some(Value::Bytes(b)) => Ok(String::from_utf8_lossy(b).into_owned()),
        Some(other) => {
            if let Some(fs) = vm.call_special(other, "__fspath__", vec![])? {
                return vm.str_of(&fs);
            }
            Err(type_err(format!(
                "expected str, bytes or os.PathLike object, not {}",
                vm.type_name(other)
            )))
        }
    }
}

pub fn make_os(vm: &mut Vm) -> Value {
    let m = new_module("_os");
    set_fn(&m, "getcwd", |vm, _| Ok(Value::string(vm.host.cwd())));
    set_fn(&m, "chdir", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        vm.host.chdir(&p).map_err(|e| os_error(e.kind, &p))?;
        Ok(Value::None)
    });
    set_fn(&m, "listdir", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        let names = vm.host.list_dir(&p).map_err(|e| os_error(e.kind, &p))?;
        Ok(Value::list(names.iter().map(|n| Value::str(n)).collect()))
    });
    set_fn(&m, "mkdir", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        vm.host.mkdir(&p, false).map_err(|e| os_error(e.kind, &p))?;
        Ok(Value::None)
    });
    set_fn(&m, "rmdir", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        match vm.host.stat(&p) {
            Ok(st) if !st.is_dir => {
                return Err(os_error(cw_script_host::FsErrorKind::NotADirectory, &p))
            }
            Err(e) => return Err(os_error(e.kind, &p)),
            _ => {}
        }
        vm.host
            .remove(&p, false)
            .map_err(|e| os_error(e.kind, &p))?;
        Ok(Value::None)
    });
    set_fn(&m, "rmtree", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        vm.host.remove(&p, true).map_err(|e| os_error(e.kind, &p))?;
        Ok(Value::None)
    });
    set_fn(&m, "remove", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        match vm.host.stat(&p) {
            Ok(st) if st.is_dir => {
                return Err(os_error(cw_script_host::FsErrorKind::IsADirectory, &p))
            }
            Err(e) => return Err(os_error(e.kind, &p)),
            _ => {}
        }
        vm.host
            .remove(&p, false)
            .map_err(|e| os_error(e.kind, &p))?;
        Ok(Value::None)
    });
    set_fn(&m, "rename", |vm, a| {
        let from = path_arg(vm, a.args.first())?;
        let to = path_arg(vm, a.args.get(1))?;
        vm.host
            .rename(&from, &to)
            .map_err(|e| os_error(e.kind, &from))?;
        Ok(Value::None)
    });
    set_fn(&m, "stat", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        let follow = match a.args.get(1) {
            Some(v) => vm.truthy(v)?,
            None => true,
        };
        let st = if follow {
            vm.host.stat(&p)
        } else {
            vm.host.lstat(&p)
        }
        .map_err(|e| os_error(e.kind, &p))?;
        let kind = if st.is_symlink {
            0o120000
        } else if st.is_dir {
            0o040000
        } else {
            0o100000
        };
        let secs = st.mtime_micros as f64 / 1e6;
        Ok(Value::tuple(vec![
            Value::Int((kind | st.mode) as i64),
            Value::Int(st.inode as i64),
            Value::Int(2049),
            Value::Int(st.links as i64),
            Value::Int(1000),
            Value::Int(1000),
            Value::Int(st.size as i64),
            Value::Float(secs),
            Value::Float(secs),
            Value::Float(secs),
        ]))
    });
    set_fn(&m, "exists", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        Ok(Value::Bool(vm.host.stat(&p).is_ok()))
    });
    set_fn(&m, "getpid", |vm, _| Ok(Value::Int(vm.host.pid() as i64)));
    set_fn(&m, "getppid", |_, _| Ok(Value::Int(1)));
    set_fn(&m, "getlogin", |vm, _| Ok(Value::string(vm.host.user())));
    set_fn(&m, "gethostname", |vm, _| {
        Ok(Value::string(vm.host.hostname()))
    });
    set_fn(&m, "cpu_count", |_, _| Ok(Value::Int(4)));
    set_fn(&m, "urandom", |vm, a| {
        let n = to_int_arg(vm, a.args.first().unwrap_or(&Value::Int(0)))?;
        let mut out = Vec::with_capacity(n.max(0) as usize);
        while out.len() < n.max(0) as usize {
            out.extend_from_slice(&vm.host.random_u64().to_le_bytes());
        }
        out.truncate(n.max(0) as usize);
        Ok(Value::Bytes(Rc::new(out)))
    });
    set_fn(&m, "abspath", |vm, a| {
        let p = path_arg(vm, a.args.first())?;
        Ok(Value::string(vm.host.resolve(&p)))
    });
    let env = new_ref(Dict::new());
    for (k, v) in vm.env.clone() {
        env.borrow_mut().set_str(&k, Value::string(v));
    }
    set_val(&m, "environ", Value::Dict(env));
    set_val(&m, "user", Value::string(vm.host.user()));
    Value::Module(m)
}

// ---------- time ----------

pub fn now_secs(vm: &Vm) -> f64 {
    (vm.host.now_micros() + vm.time_offset) as f64 / 1e6
}

/// Days since 1970-01-01 to (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// (year, mon, mday, hour, min, sec, wday(Mon=0), yday(1-based)).
pub fn gmtime_tuple(secs: i64) -> [i64; 8] {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let wday = (days + 3).rem_euclid(7); // 1970-01-01 was a Thursday (Mon=0 -> 3)
    let yday = days - days_from_civil(y, 1, 1) + 1;
    [y, mo, d, rem / 3600, rem % 3600 / 60, rem % 60, wday, yday]
}

const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn strftime(fmt: &str, t: &[i64; 8], micros: i64) -> PyResult<String> {
    strftime_in(fmt, t, micros, None)
}

/// `strftime` with the names of a locale: `%a`, `%A`, `%b`, `%B`, `%p` and the
/// `%c`/`%x`/`%X` patterns are the locale's when one is set.
pub fn strftime_in(
    fmt: &str,
    t: &[i64; 8],
    micros: i64,
    locale: Option<&'static crate::locale_data::LocaleData>,
) -> PyResult<String> {
    let [y, mo, d, h, mi, s, wd, yd] = *t;
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        // glibc's flags: `-` drops the padding, `_` pads with spaces, `0` with
        // zeroes, `^` upper-cases what follows.
        let mut pad: Option<char> = None;
        let mut upper = false;
        let mut k = match chars.next() {
            Some(c) => c,
            None => {
                out.push('%');
                break;
            }
        };
        while matches!(k, '-' | '_' | '0' | '^' | '#') {
            match k {
                '-' => pad = Some('\0'),
                '_' => pad = Some(' '),
                '0' => pad = Some('0'),
                _ => upper = true,
            }
            k = match chars.next() {
                Some(c) => c,
                None => break,
            };
        }
        let start = out.len();
        let mon_index = ((mo - 1).rem_euclid(12)) as usize;
        // `struct_time`'s weekday counts from Monday; the tables from Sunday.
        let day_index = (wd.rem_euclid(7)) as usize;
        let mon = MONTHS[mon_index];
        let day = DAYS[day_index];
        let sunday_first = ((wd + 1).rem_euclid(7)) as usize;
        let (abday, fullday, abmon, fullmon, am_pm) = match locale {
            Some(l) => (
                l.abdays[sunday_first],
                l.days[sunday_first],
                l.abmonths[mon_index],
                l.months[mon_index],
                l.am_pm,
            ),
            None => (&day[..3], day, &mon[..3], mon, &["AM", "PM"]),
        };
        match k {
            'Y' => out.push_str(&y.to_string()),
            'y' => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            'm' => out.push_str(&format!("{mo:02}")),
            'd' => out.push_str(&format!("{d:02}")),
            'e' => out.push_str(&format!("{d:2}")),
            'H' => out.push_str(&format!("{h:02}")),
            'I' => out.push_str(&format!("{:02}", if h % 12 == 0 { 12 } else { h % 12 })),
            'M' => out.push_str(&format!("{mi:02}")),
            'S' => out.push_str(&format!("{s:02}")),
            'f' => out.push_str(&format!("{micros:06}")),
            'p' => out.push_str(if h < 12 { am_pm[0] } else { am_pm[1] }),
            'a' => out.push_str(abday),
            'A' => out.push_str(fullday),
            'b' | 'h' => out.push_str(abmon),
            'B' => out.push_str(fullmon),
            'j' => out.push_str(&format!("{yd:03}")),
            'w' => out.push_str(&((wd + 1) % 7).to_string()),
            'u' => out.push_str(&(wd + 1).to_string()),
            'Z' => out.push_str("UTC"),
            'z' => out.push_str("+0000"),
            'c' => match locale {
                Some(l) => out.push_str(&strftime_in(l.d_t_fmt, t, micros, Some(l))?),
                None => out.push_str(&format!(
                    "{} {} {:2} {h:02}:{mi:02}:{s:02} {y}",
                    abday, abmon, d
                )),
            },
            'x' => match locale {
                Some(l) => out.push_str(&strftime_in(l.d_fmt, t, micros, Some(l))?),
                None => out.push_str(&format!("{mo:02}/{d:02}/{:02}", y.rem_euclid(100))),
            },
            'X' => match locale {
                Some(l) => out.push_str(&strftime_in(l.t_fmt, t, micros, Some(l))?),
                None => out.push_str(&format!("{h:02}:{mi:02}:{s:02}")),
            },
            'r' => match locale {
                Some(l) => out.push_str(&strftime_in(l.t_fmt_ampm, t, micros, Some(l))?),
                None => out.push_str(&format!(
                    "{:02}:{mi:02}:{s:02} {}",
                    if h % 12 == 0 { 12 } else { h % 12 },
                    if h < 12 { am_pm[0] } else { am_pm[1] }
                )),
            },
            'T' => out.push_str(&format!("{h:02}:{mi:02}:{s:02}")),
            'D' => out.push_str(&format!("{mo:02}/{d:02}/{:02}", y.rem_euclid(100))),
            'F' => out.push_str(&format!("{y}-{mo:02}-{d:02}")),
            'R' => out.push_str(&format!("{h:02}:{mi:02}")),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'U' => out.push_str(&format!("{:02}", (yd + 6 - (wd + 1) % 7) / 7)),
            'W' => out.push_str(&format!("{:02}", (yd + 6 - wd) / 7)),
            'G' => out.push_str(&y.to_string()),
            'V' => {
                let wk = (yd - wd + 9) / 7;
                out.push_str(&format!("{:02}", wk.max(1)));
            }
            '%' => out.push('%'),
            other => {
                out.push('%');
                out.push(other);
            }
        }
        // Apply the flags to what this conversion wrote.
        if pad.is_some() || upper {
            let piece = out.split_off(start);
            let piece = match pad {
                Some('\0') => piece.trim_start_matches(['0', ' ']).to_string(),
                Some(' ') => piece.trim_start_matches('0').to_string(),
                Some('0') => piece.replace(' ', "0"),
                _ => piece,
            };
            let piece = if piece.is_empty() && pad == Some('\0') {
                "0".to_string()
            } else {
                piece
            };
            out.push_str(&if upper { piece.to_uppercase() } else { piece });
        }
    }
    Ok(out)
}

fn struct_time_from(vm: &mut Vm, t: [i64; 8]) -> PyResult<Value> {
    let m = vm.modules.borrow().get_str("time");
    let cls = match m {
        Some(Value::Module(m)) => m.dict.borrow().get_str("struct_time"),
        _ => None,
    };
    let tup = Value::tuple(vec![
        Value::Int(t[0]),
        Value::Int(t[1]),
        Value::Int(t[2]),
        Value::Int(t[3]),
        Value::Int(t[4]),
        Value::Int(t[5]),
        Value::Int(t[6]),
        Value::Int(t[7]),
        Value::Int(0),
    ]);
    match cls {
        Some(c) => vm.call(&c, vec![tup]),
        None => Ok(tup),
    }
}

fn tuple_from_struct(vm: &mut Vm, v: &Value) -> PyResult<[i64; 8]> {
    let items = vm.iterate(v)?;
    if items.len() < 9 {
        return Err(type_err("function takes exactly 9 arguments"));
    }
    let mut t = [0i64; 8];
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = to_int_arg(vm, &items[i])?;
    }
    Ok(t)
}

pub fn make_time(vm: &mut Vm) -> Value {
    let m = new_module("time");
    set_fn(&m, "time", |vm, _| Ok(Value::Float(now_secs(vm))));
    set_fn(&m, "time_ns", |vm, _| {
        Ok(Value::Int((vm.host.now_micros() + vm.time_offset) * 1000))
    });
    fn mono(vm: &mut Vm, _a: Args) -> PyResult<Value> {
        // Seconds since the simulated boot, advancing with the world clock.
        let t = (vm.host.now_micros() + vm.time_offset - 1_789_635_600_000_000) as f64 / 1e6;
        Ok(Value::Float(t + 1000.0))
    }
    set_fn(&m, "monotonic", mono);
    set_fn(&m, "perf_counter", mono);
    set_fn(&m, "process_time", |vm, _| {
        // Simulated CPU time: steps executed so far at a nominal 50M steps/s.
        let used = STEP_BUDGET - vm.fuel;
        Ok(Value::Float(used as f64 / 50_000_000.0))
    });
    set_fn(&m, "monotonic_ns", |vm, _| {
        Ok(Value::Int(
            (vm.host.now_micros() + vm.time_offset - 1_789_635_600_000_000) * 1000
                + 1_000_000_000_000,
        ))
    });
    set_fn(&m, "perf_counter_ns", |vm, _| {
        Ok(Value::Int(
            (vm.host.now_micros() + vm.time_offset - 1_789_635_600_000_000) * 1000
                + 1_000_000_000_000,
        ))
    });
    set_fn(&m, "sleep", |vm, a| {
        let s = match a.args.first() {
            Some(v) => crate::bfuncs::float_from(vm, v)?,
            None => {
                return Err(type_err(
                    "time.sleep() takes exactly one argument (0 given)",
                ))
            }
        };
        if s < 0.0 {
            return Err(value_err("sleep length must be non-negative"));
        }
        // Simulated time passes for this process only; the world clock is not moved.
        // With threads, the others run while this one sleeps.
        vm.thread_sleep(s)
    });
    fn gm(vm: &mut Vm, a: Args) -> PyResult<Value> {
        let secs = match a.args.first() {
            Some(Value::None) | None => now_secs(vm),
            Some(v) => crate::bfuncs::float_from(vm, v)?,
        };
        struct_time_from(vm, gmtime_tuple(secs.floor() as i64))
    }
    set_fn(&m, "gmtime", gm);
    set_fn(&m, "localtime", gm);
    set_fn(&m, "mktime", |vm, a| {
        let t = tuple_from_struct(vm, &a.args[0])?;
        let days = days_from_civil(t[0], t[1], t[2]);
        Ok(Value::Float(
            (days * 86400 + t[3] * 3600 + t[4] * 60 + t[5]) as f64,
        ))
    });
    set_fn(&m, "strftime", |vm, a| {
        let fmt = to_str_arg(vm, &a.args[0], "strftime() argument 1")?;
        let t = match a.args.get(1) {
            Some(v) => tuple_from_struct(vm, v)?,
            None => gmtime_tuple(now_secs(vm).floor() as i64),
        };
        let locale = crate::modules::localemod::current_time_locale(vm);
        Ok(Value::string(strftime_in(&fmt.s, &t, 0, locale)?))
    });
    fn asctime_of(t: &[i64; 8]) -> String {
        format!(
            "{} {} {:2} {:02}:{:02}:{:02} {}",
            &DAYS[t[6].rem_euclid(7) as usize][..3],
            &MONTHS[(t[1] - 1).rem_euclid(12) as usize][..3],
            t[2],
            t[3],
            t[4],
            t[5],
            t[0]
        )
    }
    set_fn(&m, "asctime", |vm, a| {
        let t = match a.args.first() {
            Some(v) => tuple_from_struct(vm, v)?,
            None => gmtime_tuple(now_secs(vm).floor() as i64),
        };
        Ok(Value::string(asctime_of(&t)))
    });
    set_fn(&m, "ctime", |vm, a| {
        let secs = match a.args.first() {
            Some(Value::None) | None => now_secs(vm),
            Some(v) => crate::bfuncs::float_from(vm, v)?,
        };
        Ok(Value::string(asctime_of(
            &gmtime_tuple(secs.floor() as i64),
        )))
    });
    set_val(&m, "timezone", Value::Int(0));
    set_val(&m, "altzone", Value::Int(0));
    set_val(&m, "daylight", Value::Int(0));
    set_val(
        &m,
        "tzname",
        Value::tuple(vec![Value::str("UTC"), Value::str("UTC")]),
    );
    exec_snippet(
        vm,
        &m,
        r#"
class struct_time(tuple):
    _fields = ('tm_year', 'tm_mon', 'tm_mday', 'tm_hour', 'tm_min', 'tm_sec', 'tm_wday', 'tm_yday', 'tm_isdst')
    def __repr__(self):
        return 'time.struct_time(' + ', '.join(f + '=' + repr(v) for f, v in zip(self._fields, self)) + ')'
    def __getattr__(self, name):
        if name == 'tm_zone':
            return 'UTC'
        if name == 'tm_gmtoff':
            return 0
        try:
            return self[self._fields.index(name)]
        except ValueError:
            raise AttributeError(name) from None
"#,
    );
    Value::Module(m)
}

// ---------- hashlib, platform, secrets ----------

fn md5(data: &[u8]) -> Vec<u8> {
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk
            .chunks(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(s[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    [a0, b0, c0, d0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
}

fn sha1(data: &[u8]) -> Vec<u8> {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A827999),
                1 => (b ^ c ^ d, 0x6ED9EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    h.iter().flat_map(|w| w.to_be_bytes()).collect()
}

pub fn digest(name: &str, data: &[u8]) -> Option<Vec<u8>> {
    use sha2::Digest;
    Some(match name {
        "md5" => md5(data),
        "sha1" => sha1(data),
        "sha224" => sha2::Sha224::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        _ => return None,
    })
}

pub fn make_hashlib(vm: &mut Vm) -> Value {
    let m = new_module("hashlib");
    set_fn(&m, "_digest", |vm, a| {
        let name = to_str_arg(vm, &a.args[0], "name")?;
        let data = crate::bfuncs::bytes_from(vm, a.args.get(1), None, None)?;
        match digest(&name.s.to_ascii_lowercase(), &data) {
            Some(d) => Ok(Value::Bytes(Rc::new(d))),
            None => Err(value_err(format!("unsupported hash type {}", name.s))),
        }
    });
    exec_snippet(
        vm,
        &m,
        r#"
_sizes = {'md5': (16, 64), 'sha1': (20, 64), 'sha224': (28, 64), 'sha256': (32, 64), 'sha384': (48, 128), 'sha512': (64, 128)}
class _Hash:
    def __init__(self, name, data=b''):
        if name not in _sizes:
            raise ValueError('unsupported hash type ' + name)
        self.name = name
        self.digest_size, self.block_size = _sizes[name]
        self._data = b''
        if data:
            self.update(data)
    def update(self, data):
        if isinstance(data, str):
            raise TypeError('Strings must be encoded before hashing')
        self._data += bytes(data)
    def digest(self):
        return _digest(self.name, self._data)
    def hexdigest(self):
        return self.digest().hex()
    def copy(self):
        h = _Hash(self.name)
        h._data = self._data
        return h
    def __repr__(self):
        return '<' + self.name + ' _hashlib.HASH object @ 0x7f0000000000>'
def new(name, data=b''):
    return _Hash(name.lower(), data)
def md5(data=b'', **kw): return _Hash('md5', data)
def sha1(data=b'', **kw): return _Hash('sha1', data)
def sha224(data=b'', **kw): return _Hash('sha224', data)
def sha256(data=b'', **kw): return _Hash('sha256', data)
def sha384(data=b'', **kw): return _Hash('sha384', data)
def sha512(data=b'', **kw): return _Hash('sha512', data)
algorithms_guaranteed = {'md5', 'sha1', 'sha224', 'sha256', 'sha384', 'sha512'}
algorithms_available = algorithms_guaranteed
"#,
    );
    Value::Module(m)
}

pub fn make_platform(vm: &mut Vm) -> Value {
    let m = new_module("platform");
    set_fn(&m, "system", |vm, _| {
        Ok(Value::str(match vm.host.os_family().as_str() {
            "windows" => "Windows",
            "macos" | "darwin" => "Darwin",
            _ => "Linux",
        }))
    });
    set_fn(&m, "machine", |_, _| Ok(Value::str("x86_64")));
    set_fn(&m, "processor", |_, _| Ok(Value::str("x86_64")));
    set_fn(&m, "python_version", |_, _| Ok(Value::str("3.12.3")));
    set_fn(&m, "python_implementation", |_, _| {
        Ok(Value::str("CPython"))
    });
    set_fn(&m, "node", |vm, _| Ok(Value::string(vm.host.hostname())));
    set_fn(&m, "release", |_, _| Ok(Value::str("6.8.0")));
    set_fn(&m, "version", |_, _| Ok(Value::str("#1 SMP")));
    set_fn(&m, "platform", |_, _| {
        Ok(Value::str("Linux-6.8.0-x86_64-with-glibc2.39"))
    });
    set_fn(&m, "python_version_tuple", |_, _| {
        Ok(Value::tuple(vec![
            Value::str("3"),
            Value::str("12"),
            Value::str("3"),
        ]))
    });
    let _ = vm;
    Value::Module(m)
}

pub fn make_secrets(vm: &mut Vm) -> Value {
    let m = new_module("secrets");
    set_fn(&m, "token_bytes", |vm, a| {
        let n = match a.args.first() {
            Some(Value::None) | None => 32,
            Some(v) => to_int_arg(vm, v)?,
        };
        let mut out = vec![];
        while (out.len() as i64) < n {
            out.extend_from_slice(&vm.host.random_u64().to_le_bytes());
        }
        out.truncate(n.max(0) as usize);
        Ok(Value::Bytes(Rc::new(out)))
    });
    set_fn(&m, "randbelow", |vm, a| {
        let n = to_int_arg(vm, a.args.first().unwrap_or(&Value::Int(0)))?;
        if n <= 0 {
            return Err(value_err("Upper bound must be positive."));
        }
        Ok(Value::Int((vm.host.random_u64() % n as u64) as i64))
    });
    exec_snippet(
        vm,
        &m,
        r#"
import base64 as _b64
def token_hex(nbytes=None):
    return token_bytes(nbytes).hex()
def token_urlsafe(nbytes=None):
    return _b64.urlsafe_b64encode(token_bytes(nbytes)).rstrip(b'=').decode('ascii')
def choice(seq):
    return seq[randbelow(len(seq))]
def compare_digest(a, b):
    return a == b
"#,
    );
    Value::Module(m)
}
