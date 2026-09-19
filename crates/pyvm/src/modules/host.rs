//! `_cw`: the private bridge the pure-Python network and process modules
//! (`socket`, `http.client`, `urllib.request`, `subprocess`, `os.system`) are
//! written against. Every call goes to the [`cw_script_host::ScriptHost`], so
//! requests travel the world's network and children run in the machine's shell.
use super::{new_module, set_fn};
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use cw_script_host::{
    FsErrorKind, HttpRequest, NetError, NetErrorKind, SpawnProgram, SpawnRequest,
};

fn text(vm: &Vm, v: &Value, what: &str) -> PyResult<String> {
    Ok(to_str_arg(vm, v, what)?.s.clone())
}
fn bytes_of(v: &Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b.to_vec(),
        Value::ByteArray(b) => b.borrow().clone(),
        Value::Str(s) => s.s.as_bytes().to_vec(),
        _ => vec![],
    }
}
fn pairs(vm: &mut Vm, v: &Value) -> PyResult<Vec<(String, String)>> {
    let mut out = vec![];
    for item in vm.iterate(v)? {
        let kv = vm.iterate(&item)?;
        if kv.len() == 2 {
            let k = vm.str_of(&kv[0])?;
            let v = vm.str_of(&kv[1])?;
            out.push((k, v));
        }
    }
    Ok(out)
}
/// The `OSError` subclass a failed network operation raises, as CPython's
/// socket layer would (`[Errno 111] Connection refused`).
pub fn net_err(e: &NetError) -> Box<PyErr> {
    let cls = match e.kind {
        NetErrorKind::Refused => "ConnectionRefusedError",
        NetErrorKind::Reset => "ConnectionResetError",
        NetErrorKind::TimedOut => "TimeoutError",
        NetErrorKind::Denied => "PermissionError",
        _ => "OSError",
    };
    if e.kind == NetErrorKind::TimedOut {
        return err_args(cls, vec![Value::str("timed out")]);
    }
    err_args(
        cls,
        vec![
            Value::Int(e.kind.errno() as i64),
            Value::str(e.kind.strerror()),
        ],
    )
}

fn micros_of(vm: &mut Vm, v: Option<&Value>) -> PyResult<Option<u64>> {
    match v {
        None | Some(Value::None) => Ok(None),
        Some(v) => {
            let s = crate::bfuncs::float_from(vm, v)?;
            Ok(Some((s.max(0.0) * 1e6) as u64))
        }
    }
}

pub fn make(_vm: &mut Vm) -> Value {
    let m = new_module("_cw");
    // http(method, url, headers, body, timeout) -> (status, headers, body)
    set_fn(&m, "http", |vm, a| {
        let method = text(vm, &a.args[0], "method")?;
        let url = text(vm, &a.args[1], "url")?;
        let headers = pairs(vm, &a.args[2])?;
        let body = bytes_of(&a.args[3]);
        let timeout_micros = micros_of(vm, a.args.get(4))?;
        let req = HttpRequest {
            method,
            url,
            headers,
            body,
            timeout_micros,
        };
        match vm.host.http(&req) {
            Ok(r) => {
                vm.time_offset += r.elapsed_micros as i64;
                let hs: Vec<Value> = r
                    .headers
                    .iter()
                    .map(|(k, v)| Value::tuple(vec![Value::str(k), Value::str(v)]))
                    .collect();
                Ok(Value::tuple(vec![
                    Value::Int(r.status as i64),
                    Value::list(hs),
                    Value::Bytes(std::rc::Rc::new(r.body)),
                ]))
            }
            Err(e) => {
                if let Some(t) = req.timeout_micros {
                    if e.kind == NetErrorKind::TimedOut {
                        vm.time_offset += t as i64;
                    }
                }
                Err(net_err(&e))
            }
        }
    });
    // resolve(name) -> [address, ...]
    set_fn(&m, "resolve", |vm, a| {
        let name = text(vm, &a.args[0], "host")?;
        match vm.host.resolve_host(&name) {
            Ok(v) => Ok(Value::list(v.iter().map(|s| Value::str(s)).collect())),
            Err(e) => Err(net_err(&e)),
        }
    });
    // connect(host, port) -> (remote_addr, remote_port, local_addr, local_port)
    set_fn(&m, "connect", |vm, a| {
        let host = text(vm, &a.args[0], "host")?;
        let port = to_int_arg(vm, &a.args[1])?;
        match vm.host.tcp_connect(&host, port as u16) {
            Ok(c) => {
                vm.time_offset += c.elapsed_micros as i64;
                Ok(Value::tuple(vec![
                    Value::str(&c.remote_address),
                    Value::Int(c.remote_port as i64),
                    Value::str(&c.local_address),
                    Value::Int(c.local_port as i64),
                ]))
            }
            Err(e) => Err(net_err(&e)),
        }
    });
    set_fn(&m, "local_address", |vm, _| {
        Ok(Value::string(vm.host.local_address()))
    });
    // spawn(program, shell, stdin, cwd, env) -> (stdout, stderr, returncode, seconds)
    set_fn(&m, "spawn", |vm, a| {
        let shell = match a.args.get(1) {
            Some(v) => vm.truthy(v)?,
            None => false,
        };
        let program = if shell {
            SpawnProgram::Shell(vm.str_of(&a.args[0])?)
        } else {
            let mut argv = vec![];
            for v in vm.iterate(&a.args[0])? {
                argv.push(vm.str_of(&v)?);
            }
            SpawnProgram::Argv(argv)
        };
        let stdin = match a.args.get(2) {
            None | Some(Value::None) => String::new(),
            Some(v @ (Value::Bytes(_) | Value::ByteArray(_))) => {
                String::from_utf8_lossy(&bytes_of(v)).into_owned()
            }
            Some(v) => vm.str_of(v)?,
        };
        let cwd = match a.args.get(3) {
            None | Some(Value::None) => None,
            Some(v) => Some(vm.str_of(v)?),
        };
        let env = match a.args.get(4) {
            None | Some(Value::None) => None,
            Some(v) => Some(pairs(vm, v)?),
        };
        let first = match &program {
            SpawnProgram::Shell(_) => "/bin/sh".to_string(),
            SpawnProgram::Argv(v) => v.first().cloned().unwrap_or_default(),
        };
        let req = SpawnRequest {
            program,
            stdin,
            cwd: cwd.clone(),
            env,
        };
        crate::io::flush_all(vm);
        match vm.host.spawn(&req) {
            Ok(o) => {
                vm.time_offset += o.elapsed_micros as i64;
                Ok(Value::tuple(vec![
                    Value::string(o.stdout),
                    Value::string(o.stderr),
                    Value::Int(o.exit_code as i64),
                    Value::Float(o.elapsed_micros as f64 / 1e6),
                ]))
            }
            // The Python layer checks `cwd` before spawning, so a failure here
            // names the program, as CPython's `_execute_child` does.
            Err(e) => {
                let _ = cwd;
                let kind = match e.kind {
                    FsErrorKind::NotADirectory => FsErrorKind::NotADirectory,
                    FsErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
                    _ => FsErrorKind::NotFound,
                };
                Err(crate::io::os_error(kind, &first))
            }
        }
    });
    // Output a child wrote to the descriptors it inherited from us.
    set_fn(&m, "child_output", |vm, a| {
        let out = vm.str_of(&a.args[0])?;
        let err = vm.str_of(&a.args[1])?;
        vm.write_child_stdout(&out);
        vm.write_stderr(&err);
        Ok(Value::None)
    });
    // advance(seconds): virtual time passes for this process (waiting on I/O).
    set_fn(&m, "advance", |vm, a| {
        let s = crate::bfuncs::float_from(vm, &a.args[0])?;
        vm.time_offset += (s.max(0.0) * 1e6).ceil() as i64;
        Ok(Value::None)
    });
    Value::Module(m)
}
