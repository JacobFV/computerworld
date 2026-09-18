//! `open()`, file objects and the standard streams, backed by the host VFS.
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use cw_script_host::FsErrorKind;
use std::rc::Rc;

pub fn os_error(kind: FsErrorKind, path: &str) -> Box<PyErr> {
    let cls = match kind {
        FsErrorKind::NotFound => "FileNotFoundError",
        FsErrorKind::PermissionDenied => "PermissionError",
        FsErrorKind::IsADirectory => "IsADirectoryError",
        FsErrorKind::NotADirectory => "NotADirectoryError",
        FsErrorKind::Exists => "FileExistsError",
        FsErrorKind::NotEmpty | FsErrorKind::Invalid => "OSError",
    };
    err_args(
        cls,
        vec![
            Value::Int(kind.errno() as i64),
            Value::str(kind.strerror()),
            Value::str(path),
        ],
    )
}

pub fn std_file(n: u8) -> Value {
    let (name, mode) = match n {
        0 => ("<stdin>", "r"),
        1 => ("<stdout>", "w"),
        _ => ("<stderr>", "w"),
    };
    Value::File(new_ref(FileObj {
        path: name.into(),
        name: Value::str(name),
        mode: mode.into(),
        binary: false,
        readable: n == 0,
        writable: n != 0,
        append: false,
        closed: false,
        data: vec![],
        pos: 0,
        std: Some(n),
        dirty: false,
    }))
}

/// `sys.stdout` etc. as currently bound (programs may replace them).
pub fn sys_stream(vm: &mut Vm, name: &str) -> Value {
    if let Some(Value::Module(m)) = vm.modules.borrow().get_str("sys") {
        if let Some(v) = m.dict.borrow().get_str(name) {
            return v;
        }
    }
    std_file(match name {
        "stdin" => 0,
        "stdout" => 1,
        _ => 2,
    })
}

pub fn write_to(vm: &mut Vm, file: &Value, s: &str) -> PyResult<()> {
    if let Value::File(f) = file {
        return file_write(vm, f, &Value::str(s)).map(|_| ());
    }
    let w = vm.getattr_str(file, "write")?;
    vm.call(&w, vec![Value::str(s)])?;
    Ok(())
}

fn file_write(vm: &mut Vm, f: &Ref<FileObj>, v: &Value) -> PyResult<usize> {
    let mut fo = f.borrow_mut();
    if fo.closed {
        return Err(value_err("I/O operation on closed file."));
    }
    if !fo.writable {
        return Err(err("io.UnsupportedOperation", "not writable"));
    }
    let (bytes, n): (Vec<u8>, usize) = if fo.binary {
        match v {
            Value::Bytes(b) => ((**b).clone(), b.len()),
            Value::ByteArray(b) => (b.borrow().clone(), b.borrow().len()),
            other => {
                return Err(type_err(format!(
                    "a bytes-like object is required, not '{}'",
                    vm.type_name(other)
                )))
            }
        }
    } else {
        match v {
            Value::Str(s) => (s.s.as_bytes().to_vec(), s.nchars),
            other => {
                return Err(type_err(format!(
                    "write() argument must be str, not {}",
                    vm.type_name(other)
                )))
            }
        }
    };
    match fo.std {
        Some(1) => {
            drop(fo);
            vm.write_stdout(&String::from_utf8_lossy(&bytes));
        }
        Some(2) => {
            drop(fo);
            vm.write_stderr(&String::from_utf8_lossy(&bytes));
        }
        _ => {
            fo.data.extend_from_slice(&bytes);
            fo.dirty = true;
            if fo.data.len() > 1 << 16 {
                drop(fo);
                flush(vm, f)?;
            }
        }
    }
    Ok(n)
}

pub fn flush(vm: &mut Vm, f: &Ref<FileObj>) -> PyResult<()> {
    let (path, data) = {
        let mut fo = f.borrow_mut();
        if fo.std.is_some() || !fo.writable || !fo.dirty {
            return Ok(());
        }
        fo.dirty = false;
        (fo.path.clone(), std::mem::take(&mut fo.data))
    };
    vm.host
        .write_file(&path, &data, true)
        .map_err(|e| os_error(e.kind, &path))
}

pub fn flush_all(vm: &mut Vm) {
    let files = std::mem::take(&mut vm.open_files);
    for f in &files {
        let _ = flush(vm, f);
    }
}

fn read_stdin_rest(vm: &mut Vm) -> String {
    let rest = vm.stdin[vm.stdin_pos..].to_string();
    vm.stdin_pos = vm.stdin.len();
    rest
}

pub fn readline(vm: &mut Vm, f: &Ref<FileObj>, limit: i64) -> PyResult<Value> {
    let std = f.borrow().std;
    if std == Some(0) {
        let rest = &vm.stdin[vm.stdin_pos..];
        let end = match rest.find('\n') {
            Some(p) => p + 1,
            None => rest.len(),
        };
        let mut end = end;
        if limit >= 0 {
            let mut chars = 0;
            for (i, c) in rest.char_indices() {
                if chars == limit as usize {
                    end = end.min(i);
                    break;
                }
                chars += 1;
                let _ = c;
            }
        }
        let line = rest[..end].to_string();
        vm.stdin_pos += end;
        return Ok(Value::string(line));
    }
    let mut fo = f.borrow_mut();
    if fo.closed {
        return Err(value_err("I/O operation on closed file."));
    }
    if !fo.readable {
        return Err(err("io.UnsupportedOperation", "not readable"));
    }
    let start = fo.pos;
    let rest = &fo.data[start..];
    let mut end = match rest.iter().position(|b| *b == b'\n') {
        Some(p) => p + 1,
        None => rest.len(),
    };
    if limit >= 0 && (limit as usize) < end {
        end = limit as usize;
    }
    let line = rest[..end].to_vec();
    fo.pos += end;
    if fo.binary {
        Ok(Value::Bytes(Rc::new(line)))
    } else {
        Ok(Value::string(String::from_utf8_lossy(&line).into_owned()))
    }
}

pub fn b_open(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(
        &mut a,
        "open",
        &[
            "file",
            "mode",
            "buffering",
            "encoding",
            "errors",
            "newline",
            "closefd",
            "opener",
        ],
        1,
    )?;
    let file = p[0].clone().unwrap();
    let path = match &file {
        Value::Str(s) => s.s.clone(),
        Value::Int(fd) if (0..=2).contains(fd) => return Ok(std_file(*fd as u8)),
        other => {
            // os.PathLike
            if let Some(fs) = vm.call_special(other, "__fspath__", vec![])? {
                vm.str_of(&fs)?
            } else {
                return Err(type_err(format!(
                    "expected str, bytes or os.PathLike object, not {}",
                    vm.type_name(other)
                )));
            }
        }
    };
    let mode = match &p[1] {
        Some(Value::Str(m)) => m.s.clone(),
        None => "r".into(),
        Some(other) => {
            return Err(type_err(format!(
                "open() argument 'mode' must be str, not {}",
                vm.type_name(other)
            )))
        }
    };
    let encoding = match &p[3] {
        Some(Value::Str(e)) => Some(e.s.clone()),
        _ => None,
    };
    let valid = mode.chars().all(|c| "rwxabt+".contains(c))
        && mode.chars().filter(|c| "rwxa".contains(*c)).count() == 1;
    if !valid {
        return Err(value_err(format!("invalid mode: '{mode}'")));
    }
    let binary = mode.contains('b');
    if binary && encoding.is_some() {
        return Err(value_err("binary mode doesn't take an encoding argument"));
    }
    let plus = mode.contains('+');
    let (readable, writable, append) = match mode.chars().find(|c| "rwxa".contains(*c)) {
        Some('r') => (true, plus, false),
        Some('w') | Some('x') => (plus, true, false),
        _ => (plus, true, true),
    };
    let mut data = vec![];
    if mode.contains('r') {
        data = vm
            .host
            .read_file(&path)
            .map_err(|e| os_error(e.kind, &path))?;
        if !binary {
            let enc = encoding.clone().unwrap_or_else(|| "utf-8".into());
            let text = crate::bfuncs::decode_bytes(&data, &enc, "strict")?;
            data = text.replace("\r\n", "\n").into_bytes();
        }
    } else {
        if mode.contains('x') && vm.host.stat(&path).is_ok() {
            return Err(os_error(FsErrorKind::Exists, &path));
        }
        if let Ok(st) = vm.host.stat(&path) {
            if st.is_dir {
                return Err(os_error(FsErrorKind::IsADirectory, &path));
            }
        }
        // Create (and for 'w', truncate) now, as CPython does at open().
        vm.host
            .write_file(&path, b"", append)
            .map_err(|e| os_error(e.kind, &path))?;
    }
    let fo = new_ref(FileObj {
        path: path.clone(),
        name: file,
        mode: mode.clone(),
        binary,
        readable,
        writable,
        append,
        closed: false,
        data: if readable && !writable { data } else { vec![] },
        pos: 0,
        std: None,
        dirty: false,
    });
    if writable {
        vm.open_files.push(fo.clone());
    }
    Ok(Value::File(fo))
}

fn file_of(v: &Value) -> PyResult<Ref<FileObj>> {
    match v {
        Value::File(f) => Ok(f.clone()),
        _ => Err(type_err("expected a file object")),
    }
}

fn f_read(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let size = match a.args.get(1) {
        Some(Value::None) | None => -1,
        Some(v) => to_int_arg(vm, v)?,
    };
    if f.borrow().std == Some(0) {
        let rest = read_stdin_rest(vm);
        if size >= 0 {
            let s: String = rest.chars().take(size as usize).collect();
            vm.stdin_pos -= rest.len() - s.len();
            return Ok(Value::string(s));
        }
        return Ok(Value::string(rest));
    }
    let mut fo = f.borrow_mut();
    if fo.closed {
        return Err(value_err("I/O operation on closed file."));
    }
    if !fo.readable {
        return Err(err("io.UnsupportedOperation", "not readable"));
    }
    let start = fo.pos;
    let end = if size < 0 {
        fo.data.len()
    } else if fo.binary {
        (start + size as usize).min(fo.data.len())
    } else {
        // `size` counts characters in text mode.
        let text = String::from_utf8_lossy(&fo.data[start..]).into_owned();
        let bytes: usize = text.chars().take(size as usize).map(|c| c.len_utf8()).sum();
        start + bytes
    };
    let chunk = fo.data[start..end].to_vec();
    fo.pos = end;
    if fo.binary {
        Ok(Value::Bytes(Rc::new(chunk)))
    } else {
        Ok(Value::string(String::from_utf8_lossy(&chunk).into_owned()))
    }
}
fn f_readline(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let limit = match a.args.get(1) {
        Some(v) if !v.is_none() => to_int_arg(vm, v)?,
        _ => -1,
    };
    readline(vm, &f, limit)
}
fn f_readlines(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let mut out = vec![];
    loop {
        let line = readline(vm, &f, -1)?;
        let empty = match &line {
            Value::Str(s) => s.s.is_empty(),
            Value::Bytes(b) => b.is_empty(),
            _ => true,
        };
        if empty {
            break;
        }
        out.push(line);
    }
    Ok(Value::list(out))
}
fn f_write(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() != 2 {
        return Err(type_err(format!(
            "write() takes exactly one argument ({} given)",
            a.args.len() - 1
        )));
    }
    let f = file_of(&a.args[0])?;
    let n = file_write(vm, &f, &a.args[1])?;
    Ok(Value::Int(n as i64))
}
fn f_writelines(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    for line in vm.iterate(&a.args[1])? {
        file_write(vm, &f, &line)?;
    }
    Ok(Value::None)
}
fn f_close(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    flush(vm, &f)?;
    f.borrow_mut().closed = true;
    Ok(Value::None)
}
fn f_flush(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    flush(vm, &f)?;
    Ok(Value::None)
}
fn f_enter(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    if f.borrow().closed {
        return Err(value_err("I/O operation on closed file."));
    }
    Ok(a.args[0].clone())
}
fn f_exit(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    flush(vm, &f)?;
    f.borrow_mut().closed = true;
    Ok(Value::Bool(false))
}
fn f_iter(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(a.args[0].clone())
}
fn f_next(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let line = readline(vm, &f, -1)?;
    match &line {
        Value::Str(s) if s.s.is_empty() => Err(err_args("StopIteration", vec![])),
        Value::Bytes(b) if b.is_empty() => Err(err_args("StopIteration", vec![])),
        _ => Ok(line),
    }
}
fn f_seek(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let off = to_int_arg(vm, &a.args[1])?;
    let whence = match a.args.get(2) {
        Some(v) => to_int_arg(vm, v)?,
        None => 0,
    };
    let mut fo = f.borrow_mut();
    let len = fo.data.len() as i64;
    let newpos = match whence {
        0 => off,
        1 => fo.pos as i64 + off,
        _ => len + off,
    };
    fo.pos = newpos.clamp(0, len) as usize;
    Ok(Value::Int(fo.pos as i64))
}
fn f_tell(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let fo = f.borrow();
    Ok(Value::Int(if fo.writable && !fo.readable {
        fo.data.len() as i64
    } else {
        fo.pos as i64
    }))
}
fn f_fileno(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let f = file_of(&a.args[0])?;
    let fo = f.borrow();
    Ok(Value::Int(fo.std.map(|s| s as i64).unwrap_or(3)))
}
fn f_false(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::Bool(false))
}
fn f_readable(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Bool(file_of(&a.args[0])?.borrow().readable))
}
fn f_writable(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Bool(file_of(&a.args[0])?.borrow().writable))
}

pub fn install(vm: &mut Vm) {
    let c = vm.t.textio.clone();
    for (n, f) in [
        ("read", f_read as NativeFn),
        ("readline", f_readline),
        ("readlines", f_readlines),
        ("write", f_write),
        ("writelines", f_writelines),
        ("close", f_close),
        ("flush", f_flush),
        ("__enter__", f_enter),
        ("__exit__", f_exit),
        ("__iter__", f_iter),
        ("__next__", f_next),
        ("seek", f_seek),
        ("tell", f_tell),
        ("fileno", f_fileno),
        ("isatty", f_false),
        ("readable", f_readable),
        ("writable", f_writable),
    ] {
        add_fn(&c, n, f);
    }
}
