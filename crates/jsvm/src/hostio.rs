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

// ---------------------------------------------------------------- zlib

fn handle_new(vm: &mut Vm, b: Box<dyn std::any::Any>) -> Value {
    vm.handles.push(Some(b));
    Value::Num((vm.handles.len() - 1) as f64)
}
fn handle_mut<'a, T: 'static>(vm: &'a mut Vm, v: &Value) -> JsResult<&'a mut T> {
    let i = match v {
        Value::Num(n) => *n as usize,
        _ => usize::MAX,
    };
    match vm.handles.get_mut(i).and_then(|h| h.as_mut()) {
        Some(b) => match b.downcast_mut::<T>() {
            Some(t) => Ok(t),
            None => Err(Ctl::Throw(Value::str("invalid handle"))),
        },
        None => Err(Ctl::Throw(Value::str("invalid handle"))),
    }
}
fn zerr_obj(vm: &mut Vm, e: &cw_zlib::ZError) -> Value {
    let o = vm.new_object();
    let (msg, code) = match e {
        cw_zlib::ZError::Buf => ("unexpected end of file".to_string(), "Z_BUF_ERROR"),
        cw_zlib::ZError::Stream => ("stream error".to_string(), "Z_STREAM_ERROR"),
        cw_zlib::ZError::NeedDict(_) => ("Missing dictionary".to_string(), "Z_NEED_DICT"),
        cw_zlib::ZError::Data(_) => (e.message(), "Z_DATA_ERROR"),
    };
    o.set_prop("message", Value::string(msg), ALL);
    o.set_prop("code", Value::str(code), ALL);
    o.set_prop("errno", Value::Num(e.code() as f64), ALL);
    Value::Obj(o)
}

/// zlibDeflateNew(level, windowBits, memLevel, strategy, wrap(0 raw,1 zlib,2 gzip), dictionary)
fn b_deflate_new(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let level = vm.to_number(&a.arg(0))? as i32;
    let wb = vm.to_number(&a.arg(1))? as i32;
    let mem = vm.to_number(&a.arg(2))? as i32;
    let strategy = vm.to_number(&a.arg(3))? as i32;
    let wrap = vm.to_number(&a.arg(4))? as i32;
    let bits = match wrap {
        0 => -wb,
        2 => wb + 16,
        _ => wb,
    };
    let mut d =
        match cw_zlib::Deflater::new(level, bits, mem, strategy, cw_zlib::HashVariant::Chromium) {
            Ok(d) => d,
            Err(_) => return Ok(Value::Null),
        };
    if let Value::Obj(o) = a.arg(5) {
        if let Some(dict) = vm.typed_bytes(&o) {
            let _ = d.set_dictionary(&dict);
        }
    }
    Ok(handle_new(vm, Box::new(d)))
}

/// zlibDeflate(handle, input, flush, chunkSize) -> Buffer
fn b_deflate(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let input = bytes_arg(vm, &a.arg(1))?;
    let flush =
        cw_zlib::Flush::from_i32(vm.to_number(&a.arg(2))? as i32).unwrap_or(cw_zlib::Flush::None);
    let chunk = match a.arg(3) {
        Value::Num(n) if n >= 64.0 => n as usize,
        _ => 16384,
    };
    let h = a.arg(0);
    let d: &mut cw_zlib::Deflater = handle_mut(vm, &h)?;
    let mut call = 0;
    let out = cw_zlib::deflate_all(
        d,
        &input,
        flush,
        &cw_zlib::OutputSchedule::node(chunk),
        &mut call,
    );
    match out {
        Ok(o) => Ok(vm.make_buffer(o)),
        Err(e) => {
            let err = zerr_obj(vm, &e);
            Err(Ctl::Throw(err))
        }
    }
}

/// zlibParams(handle, level, strategy) -> Buffer (what the switch flushed)
fn b_deflate_params(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let level = vm.to_number(&a.arg(1))? as i32;
    let strategy = vm.to_number(&a.arg(2))? as i32;
    let h = a.arg(0);
    let d: &mut cw_zlib::Deflater = handle_mut(vm, &h)?;
    let out = d.params(level, strategy).unwrap_or_default();
    Ok(vm.make_buffer(out))
}

struct JsInflate {
    inf: cw_zlib::Inflater,
    window_bits: i32,
    dict: Option<Vec<u8>>,
    /// gunzip: continue with another member after one ends.
    multi: bool,
    ended: bool,
}

/// zlibInflateNew(windowBits (already wrapped: raw negative, gzip +16, auto +32), multi, dictionary)
fn b_inflate_new(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let wb = vm.to_number(&a.arg(0))? as i32;
    let multi = a.arg(1).truthy();
    let dict = match a.arg(2) {
        Value::Obj(o) => vm.typed_bytes(&o),
        _ => None,
    };
    let mut inf = match cw_zlib::Inflater::new(wb) {
        Ok(i) => i,
        Err(_) => return Ok(Value::Null),
    };
    if wb < 0 {
        if let Some(d) = &dict {
            let _ = inf.set_dictionary(d);
        }
    }
    let h = JsInflate {
        inf,
        window_bits: wb,
        dict,
        multi,
        ended: false,
    };
    Ok(handle_new(vm, Box::new(h)))
}

/// zlibInflate(handle, input, finish) -> { out: Buffer, ended } or throws { message, code, errno }
fn b_inflate(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let input = bytes_arg(vm, &a.arg(1))?;
    let finish = a.arg(2).truthy();
    let hv = a.arg(0);
    let r: Result<(Vec<u8>, bool), cw_zlib::ZError> = {
        let h: &mut JsInflate = handle_mut(vm, &hv)?;
        (|| {
            let mut out = vec![];
            let mut feed = input.clone();
            loop {
                if h.ended {
                    return Ok((out, true));
                }
                let mut p = h.inf.inflate(&feed, &mut out, usize::MAX)?;
                feed.clear();
                if let cw_zlib::Progress::NeedDict(_) = p {
                    match h.dict.clone() {
                        Some(d) => {
                            h.inf.set_dictionary(&d)?;
                            p = h.inf.inflate(&[], &mut out, usize::MAX)?;
                        }
                        None => return Err(cw_zlib::ZError::NeedDict(0)),
                    }
                }
                match p {
                    cw_zlib::Progress::End => {
                        let rest = h.inf.take_unconsumed();
                        if h.multi && !rest.is_empty() && !rest.iter().all(|b| *b == 0) {
                            h.inf = cw_zlib::Inflater::new(h.window_bits)?;
                            feed = rest;
                            continue;
                        }
                        h.ended = true;
                        return Ok((out, true));
                    }
                    _ => {
                        if finish {
                            return Err(cw_zlib::ZError::Buf);
                        }
                        return Ok((out, false));
                    }
                }
            }
        })()
    };
    match r {
        Ok((out, ended)) => {
            let o = vm.new_object();
            let b = vm.make_buffer(out);
            o.set_prop("out", b, ALL);
            o.set_prop("ended", Value::Bool(ended), ALL);
            Ok(Value::Obj(o))
        }
        Err(e) => {
            let err = zerr_obj(vm, &e);
            Err(Ctl::Throw(err))
        }
    }
}

fn b_handle_close(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Num(n) = a.arg(0) {
        if let Some(slot) = vm.handles.get_mut(n as usize) {
            *slot = None;
        }
    }
    Ok(Value::Undefined)
}

/// brotliCompress(input, quality, lgwin, mode, sizeHint) -> Buffer
fn b_brotli_compress(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let input = bytes_arg(vm, &a.arg(0))?;
    let q = vm.to_number(&a.arg(1))? as u32;
    let w = vm.to_number(&a.arg(2))? as u32;
    let m = vm.to_number(&a.arg(3))? as u32;
    let hint = match a.arg(4) {
        Value::Num(n) if n > 0.0 => n as usize,
        _ => 0,
    };
    let out = cw_zlib::brotli_compress(&input, q, w, m, hint);
    Ok(vm.make_buffer(out))
}

/// brotliDecompress(input) -> Buffer or throws { message, code, errno }
fn b_brotli_decompress(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let input = bytes_arg(vm, &a.arg(0))?;
    match cw_zlib::brotli_decompress(&input) {
        Ok(o) => Ok(vm.make_buffer(o)),
        Err(m) => {
            let o = vm.new_object();
            let (code, errno) = if m.contains("end of file") {
                ("ERR_BUF_ERROR", -5.0)
            } else {
                ("ERR__ERROR_FORMAT_PADDING_1", -14.0)
            };
            o.set_prop("message", Value::string(m), ALL);
            o.set_prop("code", Value::str(code), ALL);
            o.set_prop("errno", Value::Num(errno), ALL);
            Err(Ctl::Throw(Value::Obj(o)))
        }
    }
}

fn b_crc32(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let data = bytes_arg(vm, &a.arg(0))?;
    let start = match a.arg(1) {
        Value::Num(n) => n as u32,
        _ => 0,
    };
    Ok(Value::Num(cw_zlib::crc32(start, &data) as f64))
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
        ("zlibDeflateNew", 6, b_deflate_new),
        ("zlibDeflate", 4, b_deflate),
        ("zlibParams", 3, b_deflate_params),
        ("zlibInflateNew", 3, b_inflate_new),
        ("zlibInflate", 3, b_inflate),
        ("handleClose", 1, b_handle_close),
        ("brotliCompress", 5, b_brotli_compress),
        ("brotliDecompress", 1, b_brotli_decompress),
        ("crc32", 2, b_crc32),
    ];
    for (n, l, f) in fns {
        vm.method(b, n, *l, *f);
    }
}
