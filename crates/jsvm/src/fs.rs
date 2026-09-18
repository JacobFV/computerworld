//! The `fs` module over the ScriptHost's virtual filesystem.

use crate::builtins::str_arg;
use crate::value::*;
use crate::vm::*;
use cw_script_host::FsErrorKind;

fn errno(k: FsErrorKind) -> i32 {
    -k.errno()
}

fn lower_strerror(k: FsErrorKind) -> &'static str {
    match k {
        FsErrorKind::NotFound => "no such file or directory",
        FsErrorKind::Exists => "file already exists",
        FsErrorKind::NotADirectory => "not a directory",
        FsErrorKind::IsADirectory => "illegal operation on a directory",
        FsErrorKind::NotEmpty => "directory not empty",
        FsErrorKind::PermissionDenied => "permission denied",
        FsErrorKind::Invalid => "invalid argument",
    }
}

/// Node's error object for a failed fs call.
pub fn fs_error(
    vm: &mut Vm,
    k: FsErrorKind,
    syscall: &str,
    path: &str,
    dest: Option<&str>,
) -> Value {
    let msg = match dest {
        Some(d) => format!(
            "{}: {}, {syscall} '{path}' -> '{d}'",
            k.code(),
            lower_strerror(k)
        ),
        None if syscall == "read" => format!("{}: {}, {syscall}", k.code(), lower_strerror(k)),
        None => format!("{}: {}, {syscall} '{path}'", k.code(), lower_strerror(k)),
    };
    let e = vm.make_error(ErrKind::Error, &msg);
    e.set_prop("errno", Value::Num(errno(k) as f64), ALL);
    e.set_prop("code", Value::str(k.code()), ALL);
    e.set_prop("syscall", Value::str(syscall), ALL);
    if syscall != "read" {
        e.set_prop("path", Value::string(path.to_string()), ALL);
    }
    if let Some(d) = dest {
        e.set_prop("dest", Value::string(d.to_string()), ALL);
    }
    Value::Obj(e)
}

/// Rewrites the error's header/frames to look like Node's internal fs.
fn decorate(vm: &mut Vm, e: &Value, arrow: &str, internal: &[&str]) {
    if let Value::Obj(o) = e {
        let (user, _) = vm.stack_frames(true);
        // Drop the native `Object.xxx (<anonymous>)` frame(s).
        let user: Vec<String> = user
            .into_iter()
            .filter(|f| !f.ends_with("(<anonymous>)"))
            .collect();
        let mut frames: Vec<String> = internal.iter().map(|s| s.to_string()).collect();
        frames.extend(user);
        for t in vm.tail_frames() {
            frames.push(t.to_string());
        }
        frames.truncate(vm.stack_limit);
        if let Kind::Error(ed) = &mut o.borrow_mut().kind {
            ed.frames = frames;
            ed.arrow = Some(arrow.to_string());
        }
    }
}

fn throw_fs(
    vm: &mut Vm,
    k: FsErrorKind,
    syscall: &str,
    path: &str,
    dest: Option<&str>,
    arrow: &str,
    internal: &[&str],
) -> Ctl {
    let e = fs_error(vm, k, syscall, path, dest);
    decorate(vm, &e, arrow, internal);
    vm.set_site_here();
    Ctl::Throw(e)
}

fn invalid_arg(vm: &mut Vm, name: &str, expected: &str, v: &Value) -> Ctl {
    let d = vm.inspect_default(v).unwrap_or_default();
    let e = vm.make_error(
        ErrKind::TypeError,
        &format!(
            "The \"{name}\" argument must be {expected}. Received {}",
            crate::node::received(v, &d)
        ),
    );
    e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
    Ctl::Throw(Value::Obj(e))
}

/// Path argument: string, Buffer or file: URL.
fn path_arg(vm: &mut Vm, v: &Value) -> JsResult<String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Obj(o) if matches!(o.borrow().kind, Kind::TypedArray { .. }) => {
            Ok(String::from_utf8_lossy(&vm.typed_bytes(o).unwrap_or_default()).into_owned())
        }
        Value::Obj(_) => {
            let href = vm.get_str(v, "href")?;
            if let Value::Str(h) = href {
                if let Some(p) = h.strip_prefix("file://") {
                    return Ok(p.to_string());
                }
            }
            Err(invalid_arg(
                vm,
                "path",
                "of type string or an instance of Buffer or URL",
                v,
            ))
        }
        _ => Err(invalid_arg(
            vm,
            "path",
            "of type string or an instance of Buffer or URL",
            v,
        )),
    }
}

/// Encoding from an options argument ('utf8' or {encoding}).
fn encoding_of(vm: &mut Vm, v: &Value) -> JsResult<Option<String>> {
    Ok(match v {
        Value::Str(s) => Some(s.to_string()),
        Value::Obj(_) => {
            let e = vm.get_str(v, "encoding")?;
            match e {
                Value::Str(s) => Some(s.to_string()),
                _ => None,
            }
        }
        _ => None,
    })
}

fn flag_of(vm: &mut Vm, v: &Value) -> JsResult<Option<String>> {
    Ok(match v {
        Value::Obj(_) => match vm.get_str(v, "flag")? {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        },
        _ => None,
    })
}

pub fn decode_bytes(vm: &mut Vm, bytes: Vec<u8>, enc: Option<&str>) -> JsResult<Value> {
    match enc {
        None | Some("buffer") => Ok(vm.make_buffer(bytes)),
        Some(e) => {
            let s = crate::nodelib::bytes_to_string(&bytes, e);
            Ok(Value::string(s))
        }
    }
}

fn data_bytes(vm: &mut Vm, v: &Value, enc: Option<&str>) -> JsResult<Vec<u8>> {
    match v {
        Value::Str(s) => Ok(crate::nodelib::string_to_bytes(s, enc.unwrap_or("utf8"))),
        Value::Obj(o)
            if matches!(
                o.borrow().kind,
                Kind::TypedArray { .. } | Kind::ArrayBuffer(_)
            ) =>
        {
            Ok(vm.typed_bytes(o).unwrap_or_default())
        }
        Value::Obj(_)
        | Value::Num(_)
        | Value::Bool(_)
        | Value::Undefined
        | Value::Null
        | Value::BigInt(_)
        | Value::Sym(_)
        | Value::Empty => Err(invalid_arg(
            vm,
            "data",
            "of type string or an instance of Buffer, TypedArray, or DataView",
            v,
        )),
    }
}

fn is_stdin_path(p: &str) -> bool {
    p == "/dev/stdin" || p == "/proc/self/fd/0"
}

fn read_file_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let enc = encoding_of(vm, &a.arg(1))?;
    let target = a.arg(0);
    if let Value::Num(fd) = target {
        if fd == 0.0 {
            let s = if vm.stdin_consumed {
                String::new()
            } else {
                vm.stdin.clone().unwrap_or_default()
            };
            vm.stdin_consumed = true;
            return decode_bytes(vm, s.into_bytes(), enc.as_deref());
        }
    }
    let p = path_arg(vm, &target)?;
    if is_stdin_path(&p) {
        let s = if vm.stdin_consumed {
            String::new()
        } else {
            vm.stdin.clone().unwrap_or_default()
        };
        vm.stdin_consumed = true;
        return decode_bytes(vm, s.into_bytes(), enc.as_deref());
    }
    match vm.host.read_file(&p) {
        Ok(b) => decode_bytes(vm, b, enc.as_deref()),
        Err(e) => {
            if e.kind == FsErrorKind::IsADirectory {
                return Err(throw_fs(
                    vm,
                    e.kind,
                    "read",
                    &p,
                    None,
                    "node:fs:797\n  return binding.read(fd, buffer, offset, length, position);\n                 ^\n",
                    &["Object.readSync (node:fs:797:18)", "tryReadSync (node:fs:426:20)", "Object.readFileSync (node:fs:529:19)"],
                ));
            }
            if enc.is_some() {
                Err(throw_fs(
                    vm,
                    e.kind,
                    "open",
                    &p,
                    None,
                    "node:fs:483\n    return binding.readFileUtf8(path, stringToFlags(options.flag));\n                   ^\n",
                    &["Object.readFileSync (node:fs:483:20)"],
                ))
            } else {
                Err(throw_fs(
                    vm,
                    e.kind,
                    "open",
                    &p,
                    None,
                    "node:fs:621\n  return binding.open(\n                 ^\n",
                    &[
                        "Object.openSync (node:fs:621:18)",
                        "Object.readFileSync (node:fs:487:35)",
                    ],
                ))
            }
        }
    }
}

fn write_impl(vm: &mut Vm, a: &Args, append: bool) -> JsResult<Value> {
    let target = a.arg(0);
    let enc = encoding_of(vm, &a.arg(2))?;
    let bytes = data_bytes(vm, &a.arg(1), enc.as_deref())?;
    if let Value::Num(fd) = target {
        let s = String::from_utf8_lossy(&bytes).into_owned();
        if fd == 1.0 {
            vm.stdout.push_str(&s);
        } else if fd == 2.0 {
            vm.stderr.push_str(&s);
        }
        return Ok(Value::Undefined);
    }
    let p = path_arg(vm, &target)?;
    if p == "/dev/stdout" {
        vm.stdout.push_str(&String::from_utf8_lossy(&bytes));
        return Ok(Value::Undefined);
    }
    if p == "/dev/stderr" {
        vm.stderr.push_str(&String::from_utf8_lossy(&bytes));
        return Ok(Value::Undefined);
    }
    let flag = flag_of(vm, &a.arg(2))?;
    let append = append || flag.as_deref().map(|f| f.starts_with('a')).unwrap_or(false);
    if flag.as_deref().map(|f| f.contains('x')).unwrap_or(false) && vm.host.stat(&p).is_ok() {
        return Err(throw_fs(
            vm,
            FsErrorKind::Exists,
            "open",
            &p,
            None,
            "node:fs:2482\n    return binding.writeFileUtf8(\n                   ^\n",
            &["Object.writeFileSync (node:fs:2482:20)"],
        ));
    }
    if let Err(e) = vm.host.write_file(&p, &bytes, append) {
        let frames: &[&str] = if append {
            &[
                "Object.writeFileSync (node:fs:2482:20)",
                "Object.appendFileSync (node:fs:2564:6)",
            ]
        } else {
            &["Object.writeFileSync (node:fs:2482:20)"]
        };
        return Err(throw_fs(
            vm,
            e.kind,
            "open",
            &p,
            None,
            "node:fs:2482\n    return binding.writeFileUtf8(\n                   ^\n",
            frames,
        ));
    }
    Ok(Value::Undefined)
}

fn write_file_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    write_impl(vm, a, false)
}

fn append_file_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    write_impl(vm, a, true)
}

fn exists_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = a.arg(0);
    let p = match &v {
        Value::Str(s) => s.to_string(),
        Value::Obj(_) => match path_arg(vm, &v) {
            Ok(p) => p,
            Err(_) => return Ok(Value::Bool(false)),
        },
        _ => return Ok(Value::Bool(false)),
    };
    Ok(Value::Bool(vm.host.stat(&p).is_ok()))
}

fn dirent(vm: &mut Vm, name: &str, parent: &str, is_dir: bool, is_link: bool) -> Value {
    let proto = crate::nodelib::dirent_proto(vm);
    let o = vm.obj_with(Some(proto), Kind::Ordinary);
    o.set_prop("name", Value::string(name.to_string()), ALL);
    o.set_prop("parentPath", Value::string(parent.to_string()), ALL);
    o.set_prop("path", Value::string(parent.to_string()), 0);
    let t = if is_link {
        3.0
    } else if is_dir {
        2.0
    } else {
        1.0
    };
    let sym = dirent_type_symbol(vm);
    o.set_sym(&sym, Value::Num(t), ALL);
    Value::Obj(o)
}

pub fn dirent_type_symbol(vm: &mut Vm) -> std::rc::Rc<Symbol> {
    if let Some((_, s)) = vm.symbol_registry.iter().find(|(k, _)| k == "%dirent_type") {
        return s.clone();
    }
    let s = std::rc::Rc::new(Symbol {
        desc: Some(JsStr::new("type")),
        private: false,
        registered: false,
    });
    vm.symbol_registry.push(("%dirent_type".into(), s.clone()));
    s
}

fn readdir_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    let opts = a.arg(1);
    let with_types = matches!(&opts, Value::Obj(_)) && vm.get_str(&opts, "withFileTypes")?.truthy();
    let recursive = matches!(&opts, Value::Obj(_)) && vm.get_str(&opts, "recursive")?.truthy();
    let names = match vm.host.list_dir(&p) {
        Ok(n) => n,
        Err(e) => {
            return Err(throw_fs(
                vm,
                e.kind,
                "scandir",
                &p,
                None,
                "node:fs:1630\n  const result = binding.readdir(\n                         ^\n",
                &["Object.readdirSync (node:fs:1630:26)"],
            ))
        }
    };
    let mut out = vec![];
    let mut stack: Vec<(String, String)> = names.into_iter().map(|n| (n, String::new())).collect();
    stack.reverse();
    while let Some((n, prefix)) = stack.pop() {
        let rel = if prefix.is_empty() {
            n.clone()
        } else {
            format!("{prefix}/{n}")
        };
        let full = format!("{}/{}", p.trim_end_matches('/'), rel);
        let st = vm.host.lstat(&full).ok();
        let is_dir = st.as_ref().map(|s| s.is_dir).unwrap_or(false);
        let is_link = st.as_ref().map(|s| s.is_symlink).unwrap_or(false);
        if with_types {
            let parent = if prefix.is_empty() {
                p.clone()
            } else {
                format!("{}/{}", p.trim_end_matches('/'), prefix)
            };
            out.push(dirent(vm, &n, &parent, is_dir, is_link));
        } else {
            out.push(Value::string(rel.clone()));
        }
        if recursive && is_dir {
            if let Ok(children) = vm.host.list_dir(&full) {
                for c in children.into_iter().rev() {
                    stack.push((c, rel.clone()));
                }
            }
        }
    }
    Ok(vm.arr(out))
}

pub fn make_stats(vm: &mut Vm, st: &cw_script_host::FileStat) -> Value {
    let proto = crate::nodelib::stats_proto(vm);
    let o = vm.obj_with(Some(proto), Kind::Ordinary);
    let mode = st.mode
        | if st.is_symlink {
            0o120000
        } else if st.is_dir {
            0o040000
        } else {
            0o100000
        };
    let ms = (st.mtime_micros / 1000) as f64;
    let fields: [(&str, f64); 14] = [
        ("dev", 2049.0),
        ("mode", mode as f64),
        ("nlink", st.links.max(1) as f64),
        ("uid", 1000.0),
        ("gid", 1000.0),
        ("rdev", 0.0),
        ("blksize", 4096.0),
        ("ino", st.inode as f64),
        ("size", if st.is_dir { 4096.0 } else { st.size as f64 }),
        (
            "blocks",
            (st.size.div_ceil(512) * if st.is_dir { 0 } else { 1 }) as f64
                + if st.is_dir { 8.0 } else { 0.0 },
        ),
        ("atimeMs", ms),
        ("mtimeMs", ms),
        ("ctimeMs", ms),
        ("birthtimeMs", ms),
    ];
    for (k, v) in fields {
        o.set_prop(k, Value::Num(v), ALL);
    }
    Value::Obj(o)
}

fn stat_impl(vm: &mut Vm, a: &Args, lstat: bool) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    let r = if lstat {
        vm.host.lstat(&p)
    } else {
        vm.host.stat(&p)
    };
    match r {
        Ok(st) => Ok(make_stats(vm, &st)),
        Err(e) => {
            let opts = a.arg(1);
            if let Value::Obj(_) = &opts {
                if matches!(vm.get_str(&opts, "throwIfNoEntry")?, Value::Bool(false))
                    && e.kind == FsErrorKind::NotFound
                {
                    return Ok(Value::Undefined);
                }
            }
            if lstat {
                Err(throw_fs(
                    vm,
                    e.kind,
                    "lstat",
                    &p,
                    None,
                    "node:fs:1770\n  const stats = binding.lstat(\n                        ^\n",
                    &["Object.lstatSync (node:fs:1770:25)"],
                ))
            } else {
                Err(throw_fs(
                    vm,
                    e.kind,
                    "stat",
                    &p,
                    None,
                    "node:fs:1794\n  const stats = binding.stat(\n                        ^\n",
                    &["Object.statSync (node:fs:1794:25)"],
                ))
            }
        }
    }
}

fn stat_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    stat_impl(vm, a, false)
}
fn lstat_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    stat_impl(vm, a, true)
}

fn mkdir_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    let opts = a.arg(1);
    let recursive = matches!(&opts, Value::Obj(_)) && vm.get_str(&opts, "recursive")?.truthy();
    let existed = vm.host.stat(&p).is_ok();
    match vm.host.mkdir(&p, recursive) {
        Ok(()) => {
            if recursive && !existed {
                Ok(Value::string(vm.host.resolve(&p)))
            } else {
                Ok(Value::Undefined)
            }
        }
        Err(e) => {
            if recursive
                && e.kind == FsErrorKind::Exists
                && vm.host.stat(&p).map(|s| s.is_dir).unwrap_or(false)
            {
                return Ok(Value::Undefined);
            }
            Err(throw_fs(
                vm,
                e.kind,
                "mkdir",
                &p,
                None,
                "node:fs:1410\n  const result = binding.mkdir(\n                         ^\n",
                &["Object.mkdirSync (node:fs:1410:26)"],
            ))
        }
    }
}

fn rm_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    let opts = a.arg(1);
    let recursive = matches!(&opts, Value::Obj(_)) && vm.get_str(&opts, "recursive")?.truthy();
    let force = matches!(&opts, Value::Obj(_)) && vm.get_str(&opts, "force")?.truthy();
    let st = match vm.host.lstat(&p) {
        Ok(s) => s,
        Err(e) => {
            if force && e.kind == FsErrorKind::NotFound {
                return Ok(Value::Undefined);
            }
            return Err(throw_fs(
                vm,
                e.kind,
                "lstat",
                &p,
                None,
                "node:internal/errors:543\n      throw error;\n      ^\n",
                &["Object.rmSync (node:fs:1281:16)"],
            ));
        }
    };
    if st.is_dir && !recursive {
        let e = vm.make_error(
            ErrKind::Error,
            &format!("Path is a directory: rm returned EISDIR (is a directory) {p}"),
        );
        e.set_hidden("name", Value::str("SystemError"));
        e.set_prop("code", Value::str("ERR_FS_EISDIR"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    }
    if let Err(e) = vm.host.remove(&p, recursive) {
        return Err(throw_fs(
            vm,
            e.kind,
            "rm",
            &p,
            None,
            "node:internal/errors:543\n      throw error;\n      ^\n",
            &["Object.rmSync (node:fs:1281:16)"],
        ));
    }
    Ok(Value::Undefined)
}

fn rmdir_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    let opts = a.arg(1);
    let recursive = matches!(&opts, Value::Obj(_)) && vm.get_str(&opts, "recursive")?.truthy();
    match vm.host.stat(&p) {
        Ok(st) if !st.is_dir => {
            return Err(throw_fs(
                vm,
                FsErrorKind::NotADirectory,
                "rmdir",
                &p,
                None,
                "node:fs:1236\n  binding.rmdir(path);\n          ^\n",
                &["Object.rmdirSync (node:fs:1236:11)"],
            ));
        }
        _ => {}
    }
    if let Err(e) = vm.host.remove(&p, recursive) {
        return Err(throw_fs(
            vm,
            e.kind,
            "rmdir",
            &p,
            None,
            "node:fs:1236\n  binding.rmdir(path);\n          ^\n",
            &["Object.rmdirSync (node:fs:1236:11)"],
        ));
    }
    Ok(Value::Undefined)
}

fn unlink_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    match vm.host.lstat(&p) {
        Ok(st) if st.is_dir => {
            return Err(throw_fs(
                vm,
                FsErrorKind::IsADirectory,
                "unlink",
                &p,
                None,
                "node:fs:2000\n  binding.unlink(getValidatedPath(path));\n          ^\n",
                &["Object.unlinkSync (node:fs:2000:11)"],
            ));
        }
        _ => {}
    }
    if let Err(e) = vm.host.remove(&p, false) {
        return Err(throw_fs(
            vm,
            e.kind,
            "unlink",
            &p,
            None,
            "node:fs:2000\n  binding.unlink(getValidatedPath(path));\n          ^\n",
            &["Object.unlinkSync (node:fs:2000:11)"],
        ));
    }
    Ok(Value::Undefined)
}

fn rename_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let from = path_arg(vm, &a.arg(0))?;
    let to = path_arg(vm, &a.arg(1))?;
    if let Err(e) = vm.host.rename(&from, &to) {
        return Err(throw_fs(
            vm,
            e.kind,
            "rename",
            &from,
            Some(&to),
            "node:fs:1073\n  binding.rename(\n          ^\n",
            &["Object.renameSync (node:fs:1073:11)"],
        ));
    }
    Ok(Value::Undefined)
}

fn copy_file_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let from = path_arg(vm, &a.arg(0))?;
    let to = path_arg(vm, &a.arg(1))?;
    let data = match vm.host.read_file(&from) {
        Ok(d) => d,
        Err(e) => {
            return Err(throw_fs(
                vm,
                e.kind,
                "copyfile",
                &from,
                Some(&to),
                "node:fs:3200\n  binding.copyFile(\n          ^\n",
                &["Object.copyFileSync (node:fs:3200:11)"],
            ))
        }
    };
    if let Err(e) = vm.host.write_file(&to, &data, false) {
        return Err(throw_fs(
            vm,
            e.kind,
            "copyfile",
            &from,
            Some(&to),
            "node:fs:3200\n  binding.copyFile(\n          ^\n",
            &["Object.copyFileSync (node:fs:3200:11)"],
        ));
    }
    Ok(Value::Undefined)
}

fn access_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    if let Err(e) = vm.host.stat(&p) {
        return Err(throw_fs(
            vm,
            e.kind,
            "access",
            &p,
            None,
            "node:fs:244\n  binding.access(getValidatedPath(path), mode);\n          ^\n",
            &["Object.accessSync (node:fs:244:11)"],
        ));
    }
    Ok(Value::Undefined)
}

fn realpath_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    if let Err(e) = vm.host.stat(&p) {
        return Err(throw_fs(vm, e.kind, "lstat", &p, None, "node:fs:2720\n    binding.lstat(base, false, undefined, true /* throwIfNoEntry */);\n            ^\n", &["Object.realpathSync (node:fs:2720:13)"]));
    }
    Ok(Value::string(vm.host.resolve(&p)))
}

fn mkdtemp_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let prefix = str_arg(vm, a, 0)?;
    let mut suffix = String::new();
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    for _ in 0..6 {
        let r = vm.host.random_u64();
        suffix.push(CH[(r % CH.len() as u64) as usize] as char);
    }
    let p = format!("{prefix}{suffix}");
    if let Err(e) = vm.host.mkdir(&p, false) {
        return Err(throw_fs(vm, e.kind, "mkdtemp", &p, None, "node:fs:2940\n  const path = binding.mkdtemp(prefix, options.encoding);\n                       ^\n", &["Object.mkdtempSync (node:fs:2940:24)"]));
    }
    Ok(Value::string(p))
}

// ---- file descriptors (a small table of open files over whole-file I/O)

fn open_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = path_arg(vm, &a.arg(0))?;
    let flags = match a.arg(1) {
        Value::Str(s) => s.to_string(),
        _ => "r".into(),
    };
    if flags.starts_with('r') {
        if let Err(e) = vm.host.read_file(&p) {
            return Err(throw_fs(
                vm,
                e.kind,
                "open",
                &p,
                None,
                "node:fs:621\n  return binding.open(\n                 ^\n",
                &["Object.openSync (node:fs:621:18)"],
            ));
        }
    } else if flags.starts_with('w') {
        if let Err(e) = vm.host.write_file(&p, b"", false) {
            return Err(throw_fs(
                vm,
                e.kind,
                "open",
                &p,
                None,
                "node:fs:621\n  return binding.open(\n                 ^\n",
                &["Object.openSync (node:fs:621:18)"],
            ));
        }
    } else if flags.starts_with('a') {
        if let Err(e) = vm.host.write_file(&p, b"", true) {
            return Err(throw_fs(
                vm,
                e.kind,
                "open",
                &p,
                None,
                "node:fs:621\n  return binding.open(\n                 ^\n",
                &["Object.openSync (node:fs:621:18)"],
            ));
        }
    }
    let fd = 3 + vm.open_fds.len();
    vm.open_fds.push(Some((p, 0)));
    Ok(Value::Num(fd as f64))
}

fn fd_path(vm: &mut Vm, v: &Value) -> Option<(usize, String, usize)> {
    let Value::Num(n) = v else { return None };
    let i = (*n as usize).checked_sub(3)?;
    match vm.open_fds.get(i) {
        Some(Some((p, pos))) => Some((i, p.clone(), *pos)),
        _ => None,
    }
}

fn bad_fd(vm: &mut Vm, syscall: &str) -> Ctl {
    let e = vm.make_error(
        ErrKind::Error,
        &format!("EBADF: bad file descriptor, {syscall}"),
    );
    e.set_prop("errno", Value::Num(-9.0), ALL);
    e.set_prop("code", Value::str("EBADF"), ALL);
    e.set_prop("syscall", Value::str(syscall), ALL);
    Ctl::Throw(Value::Obj(e))
}

fn close_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    match fd_path(vm, &a.arg(0)) {
        Some((i, _, _)) => {
            vm.open_fds[i] = None;
            Ok(Value::Undefined)
        }
        None => Err(bad_fd(vm, "close")),
    }
}

fn write_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let fdv = a.arg(0);
    let bytes = data_bytes(vm, &a.arg(1), None)?;
    if let Value::Num(n) = fdv {
        if n == 1.0 || n == 2.0 {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            if n == 1.0 {
                vm.stdout.push_str(&s);
            } else {
                vm.stderr.push_str(&s);
            }
            return Ok(Value::Num(bytes.len() as f64));
        }
    }
    match fd_path(vm, &fdv) {
        Some((_, p, _)) => {
            if let Err(e) = vm.host.write_file(&p, &bytes, true) {
                return Err(Ctl::Throw(fs_error(vm, e.kind, "write", &p, None)));
            }
            Ok(Value::Num(bytes.len() as f64))
        }
        None => Err(bad_fd(vm, "write")),
    }
}

fn read_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let fdv = a.arg(0);
    let Value::Obj(buf) = a.arg(1) else {
        return Err(invalid_arg(
            vm,
            "buffer",
            "an instance of Buffer, TypedArray, or DataView",
            &a.arg(1),
        ));
    };
    let off = vm.to_integer(&a.arg(2))?.max(0.0) as usize;
    let data: Vec<u8> = if let Value::Num(0.0) = fdv {
        let s = if vm.stdin_consumed {
            String::new()
        } else {
            vm.stdin.clone().unwrap_or_default()
        };
        vm.stdin_consumed = true;
        s.into_bytes()
    } else {
        match fd_path(vm, &fdv) {
            Some((i, p, pos)) => {
                let all = vm.host.read_file(&p).unwrap_or_default();
                let chunk = all[pos.min(all.len())..].to_vec();
                let len = match a.arg(3) {
                    Value::Num(n) => n as usize,
                    _ => chunk.len(),
                };
                let take = chunk.len().min(len);
                if let Some(Some((_, pp))) = vm.open_fds.get_mut(i) {
                    *pp += take;
                }
                chunk[..take].to_vec()
            }
            None => return Err(bad_fd(vm, "read")),
        }
    };
    let n = {
        let d = buf.borrow();
        match &d.kind {
            Kind::TypedArray {
                buf: b,
                offset,
                len,
                ..
            } => {
                let mut bb = b.borrow_mut();
                let space = len.saturating_sub(off);
                let n = data.len().min(space);
                bb[offset + off..offset + off + n].copy_from_slice(&data[..n]);
                n
            }
            _ => 0,
        }
    };
    Ok(Value::Num(n as f64))
}

fn fstat_sync(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    match fd_path(vm, &a.arg(0)) {
        Some((_, p, _)) => match vm.host.stat(&p) {
            Ok(st) => Ok(make_stats(vm, &st)),
            Err(e) => Err(Ctl::Throw(fs_error(vm, e.kind, "fstat", &p, None))),
        },
        None => Err(bad_fd(vm, "fstat")),
    }
}

fn watch(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let o = vm.new_object();
    vm.method(&o, "close", 0, |_vm, _a| Ok(Value::Undefined));
    vm.method(&o, "on", 2, |_vm, a| Ok(a.this.clone()));
    Ok(Value::Obj(o))
}

/// Wraps a sync function as a callback-style async function.
fn callback_wrapper(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = crate::promise::slots(a);
    let sync = s[0].clone();
    let mut args = a.args.clone();
    let cb = match args.last() {
        Some(f) if f.is_callable() => args.pop(),
        _ => None,
    };
    let r = vm.call(&sync, Value::Undefined, args);
    let (err, val) = match r {
        Ok(v) => (Value::Null, v),
        Err(Ctl::Throw(e)) => {
            // Errors of the callback API come from libuv: no JS stack.
            if let Value::Obj(o) = &e {
                if let Kind::Error(ed) = &mut o.borrow_mut().kind {
                    ed.frames.clear();
                    ed.arrow = None;
                }
                o.borrow_mut()
                    .props
                    .insert(Key::str("stack"), Prop::data(Value::Empty, HIDDEN));
            }
            (e, Value::Undefined)
        }
        Err(o) => return Err(o),
    };
    if let Some(cb) = cb {
        let args = if err.is_nullish() {
            vec![Value::Null, val]
        } else {
            vec![err]
        };
        let id = vm.timer_id;
        vm.timer_id += 1;
        vm.timer_seq += 1;
        let obj = vm.new_object();
        vm.timers.push(Timer {
            id,
            when: vm.elapsed_ms,
            seq: vm.timer_seq,
            callback: cb,
            args,
            interval: None,
            obj,
            immediate: true,
        });
    }
    Ok(Value::Undefined)
}

/// Wraps a sync function as a promise-returning function.
fn promise_wrapper(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = crate::promise::slots(a);
    let sync = s[0].clone();
    let p = vm.new_promise();
    // Results arrive on a later turn of the loop, like libuv completions.
    let settle = |vm: &mut Vm, p: &Obj, v: Value, rejected: bool| {
        let f = vm.native_fn_slots(
            "",
            0,
            settle_later,
            vec![Value::Obj(p.clone()), v, Value::Bool(rejected)],
        );
        let id = vm.timer_id;
        vm.timer_id += 1;
        vm.timer_seq += 1;
        let obj = vm.new_object();
        vm.timers.push(Timer {
            id,
            when: vm.elapsed_ms,
            seq: vm.timer_seq,
            callback: Value::Obj(f),
            args: vec![],
            interval: None,
            obj,
            immediate: true,
        });
    };
    match vm.call(&sync, Value::Undefined, a.args.clone()) {
        Ok(v) => settle(vm, &p, v, false),
        Err(Ctl::Throw(e)) => {
            // Promise-based APIs report async frames.
            if let Value::Obj(eo) = &e {
                if let Kind::Error(ed) = &mut eo.borrow_mut().kind {
                    let name = match &s[1] {
                        Value::Str(n) => n.to_string(),
                        _ => "readFile".into(),
                    };
                    ed.frames = vec![
                        "async open (node:internal/fs/promises:1345:25)".into(),
                        format!("async Object.{name} (node:internal/fs/promises:1996:14)"),
                    ];
                    ed.arrow = Some("node:internal/fs/promises:1345\n  return new FileHandle(await PromisePrototypeThen(\n                        ^\n".into());
                }
            }
            settle(vm, &p, e, true)
        }
        Err(o) => return Err(o),
    }
    Ok(Value::Obj(p))
}

fn settle_later(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = crate::promise::slots(a);
    if let Value::Obj(p) = &s[0] {
        if s[2].truthy() {
            vm.reject_promise(p, s[1].clone());
        } else {
            vm.resolve_promise(p, s[1].clone())?;
        }
    }
    Ok(Value::Undefined)
}

pub fn make_fs(vm: &mut Vm) -> Value {
    if let Some(Value::Obj(f)) = vm.global.own_value("%fs") {
        return Value::Obj(f);
    }
    let fs = vm.new_object();
    let fns: &[(&str, u32, NativeFn)] = &[
        ("readFileSync", 2, read_file_sync),
        ("writeFileSync", 3, write_file_sync),
        ("appendFileSync", 3, append_file_sync),
        ("existsSync", 1, exists_sync),
        ("readdirSync", 2, readdir_sync),
        ("statSync", 2, stat_sync),
        ("lstatSync", 2, lstat_sync),
        ("mkdirSync", 2, mkdir_sync),
        ("rmSync", 2, rm_sync),
        ("rmdirSync", 2, rmdir_sync),
        ("unlinkSync", 1, unlink_sync),
        ("renameSync", 2, rename_sync),
        ("copyFileSync", 3, copy_file_sync),
        ("accessSync", 2, access_sync),
        ("realpathSync", 2, realpath_sync),
        ("mkdtempSync", 2, mkdtemp_sync),
        ("openSync", 3, open_sync),
        ("closeSync", 1, close_sync),
        ("writeSync", 2, write_sync),
        ("readSync", 5, read_sync),
        ("fstatSync", 1, fstat_sync),
    ];
    let promises = vm.new_object();
    for (n, l, f) in fns {
        let sync = vm.method(&fs, n, *l, *f);
        let base = n.trim_end_matches("Sync");
        if *n == "existsSync" {
            // fs.exists(path, cb) passes a boolean only.
            let cbw = vm.native_fn_slots(base, 2, exists_cb, vec![Value::Obj(sync.clone())]);
            fs.set_hidden(base, Value::Obj(cbw));
            continue;
        }
        let cbw = vm.native_fn_slots(
            base,
            *l + 1,
            callback_wrapper,
            vec![Value::Obj(sync.clone())],
        );
        fs.set_hidden(base, Value::Obj(cbw));
        if !matches!(base, "open" | "close" | "write" | "read" | "fstat") {
            let pw = vm.native_fn_slots(
                base,
                *l,
                promise_wrapper,
                vec![Value::Obj(sync), Value::str(base)],
            );
            promises.set_hidden(base, Value::Obj(pw));
        }
    }
    vm.method(&fs, "watch", 2, watch);
    vm.method(&fs, "watchFile", 2, watch);
    let consts = crate::builtins::new_obj_from(
        vm,
        vec![
            ("F_OK", Value::Num(0.0)),
            ("R_OK", Value::Num(4.0)),
            ("W_OK", Value::Num(2.0)),
            ("X_OK", Value::Num(1.0)),
        ],
    );
    fs.set_prop("constants", Value::Obj(consts.clone()), ALL);
    promises.set_prop("constants", Value::Obj(consts), ALL);
    fs.set_prop("promises", Value::Obj(promises), ALL);
    vm.global.set_hidden("%fs", Value::Obj(fs.clone()));
    Value::Obj(fs)
}

fn exists_cb(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = crate::promise::slots(a);
    let r = vm.call(&s[0], Value::Undefined, vec![a.arg(0)])?;
    if let Some(cb) = a.args.iter().skip(1).find(|x| x.is_callable()) {
        vm.ticks.push_back((cb.clone(), vec![r]));
    }
    Ok(Value::Undefined)
}
