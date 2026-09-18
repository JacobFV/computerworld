//! Native bindings for the modules that reach outside the program: `http`,
//! `https`, `fetch`, `net`, `dns` and `child_process`. Everything goes through
//! the [`cw_script_host::ScriptHost`]: requests travel the world's network and
//! children run in the machine's own shell. The JS side (`js/*.js`) turns the
//! synchronous answers into Node's asynchronous API, delivering each completion
//! as a poll-phase I/O event at the simulated time it would arrive.
use crate::value::*;
use crate::vm::*;
use cw_script_host::{HttpRequest, NetError, SpawnProgram, SpawnRequest};

fn string_list(vm: &mut Vm, v: &Value) -> JsResult<Vec<String>> {
    let Value::Obj(o) = v else {
        return Ok(vec![]);
    };
    let n = vm.array_len(o).unwrap_or(0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let item = vm.get_str(v, &i.to_string())?;
        out.push(vm.to_str(&item)?);
    }
    Ok(out)
}
fn pairs(vm: &mut Vm, v: &Value) -> JsResult<Vec<(String, String)>> {
    let flat = string_list(vm, v)?;
    Ok(flat
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].clone(), c[1].clone()))
        .collect())
}
fn bytes_arg(vm: &mut Vm, v: &Value) -> JsResult<Vec<u8>> {
    match v {
        Value::Undefined | Value::Null => Ok(vec![]),
        Value::Str(s) => Ok(s.to_string().into_bytes()),
        Value::Obj(o) => match vm.typed_bytes(o) {
            Some(b) => Ok(b),
            None => Ok(vm.to_str(v)?.into_bytes()),
        },
        other => Ok(vm.to_str(other)?.into_bytes()),
    }
}
/// `{ code, errno, message }` for a failed network operation; the JS layer
/// builds Node's error (`connect ECONNREFUSED 10.0.0.5:443`) from it.
fn net_error_obj(vm: &mut Vm, e: &NetError) -> Value {
    let o = vm.new_object();
    let (code, errno) = match e.kind {
        cw_script_host::NetErrorKind::NameNotFound => ("ENOTFOUND", -3008.0),
        k => (k.code(), -(k.errno() as f64)),
    };
    o.set_prop("code", Value::str(code), ALL);
    o.set_prop("errno", Value::Num(errno), ALL);
    o.set_prop("message", Value::string(e.message.clone()), ALL);
    Value::Obj(o)
}

/// httpRequest(method, url, flatHeaders, body, timeoutMs) ->
/// { status, headers: flat, body: Buffer, elapsedMs } | { error }
fn b_http_request(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let method = vm.to_str(&a.arg(0))?;
    let url = vm.to_str(&a.arg(1))?;
    let headers = pairs(vm, &a.arg(2))?;
    let body = bytes_arg(vm, &a.arg(3))?;
    let timeout = match a.arg(4) {
        Value::Num(n) if n > 0.0 => Some((n * 1000.0) as u64),
        _ => None,
    };
    let req = HttpRequest {
        method,
        url,
        headers,
        body,
        timeout_micros: timeout,
    };
    let o = vm.new_object();
    match vm.host.http(&req) {
        Ok(r) => {
            o.set_prop("status", Value::Num(r.status as f64), ALL);
            let flat: Vec<Value> = r
                .headers
                .iter()
                .flat_map(|(k, v)| [Value::string(k.clone()), Value::string(v.clone())])
                .collect();
            let h = vm.arr(flat);
            o.set_prop("headers", h, ALL);
            let b = vm.make_buffer(r.body);
            o.set_prop("body", b, ALL);
            o.set_prop(
                "elapsedMs",
                Value::Num(r.elapsed_micros as f64 / 1000.0),
                ALL,
            );
        }
        Err(e) => {
            let err = net_error_obj(vm, &e);
            o.set_prop("error", err, ALL);
            let waited = match (e.kind, timeout) {
                (cw_script_host::NetErrorKind::TimedOut, Some(t)) => t as f64 / 1000.0,
                _ => 0.0,
            };
            o.set_prop("elapsedMs", Value::Num(waited), ALL);
        }
    }
    Ok(Value::Obj(o))
}

/// dnsLookup(name) -> { addresses } | { error }
fn b_dns_lookup(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let name = vm.to_str(&a.arg(0))?;
    let o = vm.new_object();
    match vm.host.resolve_host(&name) {
        Ok(addrs) => {
            let v = vm.arr(addrs.into_iter().map(Value::string).collect());
            o.set_prop("addresses", v, ALL);
        }
        Err(e) => {
            let err = net_error_obj(vm, &e);
            o.set_prop("error", err, ALL);
        }
    }
    Ok(Value::Obj(o))
}

/// tcpConnect(host, port) -> { remoteAddress, remotePort, localAddress,
/// localPort, elapsedMs } | { error }
fn b_tcp_connect(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let host = vm.to_str(&a.arg(0))?;
    let port = vm.to_number(&a.arg(1))? as u16;
    let o = vm.new_object();
    match vm.host.tcp_connect(&host, port) {
        Ok(c) => {
            o.set_prop("remoteAddress", Value::string(c.remote_address), ALL);
            o.set_prop("remotePort", Value::Num(c.remote_port as f64), ALL);
            o.set_prop("localAddress", Value::string(c.local_address), ALL);
            o.set_prop("localPort", Value::Num(c.local_port as f64), ALL);
            o.set_prop(
                "elapsedMs",
                Value::Num(c.elapsed_micros as f64 / 1000.0),
                ALL,
            );
        }
        Err(e) => {
            let err = net_error_obj(vm, &e);
            o.set_prop("error", err, ALL);
        }
    }
    Ok(Value::Obj(o))
}

fn b_local_address(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::string(vm.host.local_address()))
}

/// spawn(file, args, shellCommand, input, cwd, flatEnv) ->
/// { stdout: Buffer, stderr: Buffer, status, elapsedMs } | { error: code }
fn b_spawn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let program = match a.arg(2) {
        Value::Str(s) => SpawnProgram::Shell(s.to_string()),
        _ => {
            let mut argv = vec![vm.to_str(&a.arg(0))?];
            argv.extend(string_list(vm, &a.arg(1))?);
            SpawnProgram::Argv(argv)
        }
    };
    let input = bytes_arg(vm, &a.arg(3))?;
    let cwd = match a.arg(4) {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    };
    let env = match a.arg(5) {
        Value::Obj(_) => Some(pairs(vm, &a.arg(5))?),
        _ => None,
    };
    let req = SpawnRequest {
        program,
        stdin: String::from_utf8_lossy(&input).into_owned(),
        cwd,
        env,
    };
    let o = vm.new_object();
    match vm.host.spawn(&req) {
        Ok(out) => {
            let so = vm.make_buffer(out.stdout.into_bytes());
            let se = vm.make_buffer(out.stderr.into_bytes());
            o.set_prop("stdout", so, ALL);
            o.set_prop("stderr", se, ALL);
            o.set_prop("status", Value::Num(out.exit_code as f64), ALL);
            o.set_prop(
                "elapsedMs",
                Value::Num(out.elapsed_micros as f64 / 1000.0),
                ALL,
            );
        }
        Err(e) => {
            o.set_prop("error", Value::str(e.kind.code()), ALL);
            o.set_prop("errno", Value::Num(-(e.kind.errno() as f64)), ALL);
        }
    }
    Ok(Value::Obj(o))
}

/// scheduleIo(callback, delayMs, ...args): a poll-phase completion `delayMs`
/// of simulated time from now.
fn b_schedule_io(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let cb = a.arg(0);
    let delay = match a.arg(1) {
        Value::Num(n) if n > 0.0 => n,
        _ => 0.0,
    };
    let args: Vec<Value> = a.args.iter().skip(2).cloned().collect();
    vm.schedule_io(cb, delay, args);
    Ok(Value::Undefined)
}

/// advance(ms): synchronous waiting (`execSync`, `spawnSync`) moves the clock.
fn b_advance(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let ms = vm.to_number(&a.arg(0))?;
    if ms > 0.0 {
        let now = vm.clock();
        vm.elapsed_ms = now + ms;
    }
    Ok(Value::Undefined)
}

/// Writes a child's inherited output to our streams.
fn b_child_output(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let out = bytes_arg(vm, &a.arg(0))?;
    let err = bytes_arg(vm, &a.arg(1))?;
    vm.stdout.push_str(&String::from_utf8_lossy(&out));
    vm.stderr.push_str(&String::from_utf8_lossy(&err));
    Ok(Value::Undefined)
}

impl<'h> Vm<'h> {
    /// Queues an I/O completion for the poll phase at `now + delay_ms`.
    pub fn schedule_io(&mut self, cb: Value, delay_ms: f64, args: Vec<Value>) {
        let id = self.timer_id;
        self.timer_id += 1;
        self.timer_seq += 1;
        let obj = self.new_object();
        let when = self.clock() + delay_ms;
        self.timers.push(Timer {
            id,
            when,
            seq: self.timer_seq,
            callback: cb,
            args,
            interval: None,
            obj,
            immediate: true,
            io: true,
            dur: 0.0,
        });
    }
}

pub fn install(vm: &mut Vm, b: &Obj) {
    let fns: &[(&str, u32, NativeFn)] = &[
        ("httpRequest", 5, b_http_request),
        ("dnsLookup", 1, b_dns_lookup),
        ("tcpConnect", 2, b_tcp_connect),
        ("localAddress", 0, b_local_address),
        ("spawn", 6, b_spawn),
        ("scheduleIo", 2, b_schedule_io),
        ("advance", 1, b_advance),
        ("childOutput", 2, b_child_output),
    ];
    for (n, l, f) in fns {
        vm.method(b, n, *l, *f);
    }
}
