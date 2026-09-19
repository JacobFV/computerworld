//! Native pieces of Node's library: path, os, crypto, Buffer encodings and
//! the internal binding used by the JavaScript half (js/*.js).

use crate::builtins::str_arg;
use crate::node::{dirname, normalize};
use crate::promise::slots;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

// ---------------------------------------------------------------- encodings

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(b: &[u8], url: bool) -> String {
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = match chunk.len() {
            3 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32,
            2 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8,
            _ => (chunk[0] as u32) << 16,
        };
        let idx = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        let count = chunk.len() + 1;
        for (i, x) in idx.iter().enumerate() {
            if i < count {
                let mut c = B64[*x as usize] as char;
                if url {
                    c = match c {
                        '+' => '-',
                        '/' => '_',
                        c => c,
                    };
                }
                out.push(c);
            } else if !url {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = vec![];
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            _ => continue,
        };
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

pub fn norm_encoding(e: &str) -> String {
    match e.to_ascii_lowercase().as_str() {
        "utf8" | "utf-8" => "utf8".into(),
        "ucs2" | "ucs-2" | "utf16le" | "utf-16le" => "utf16le".into(),
        "latin1" | "binary" => "latin1".into(),
        other => other.to_string(),
    }
}

pub fn bytes_to_string(b: &[u8], enc: &str) -> String {
    match norm_encoding(enc).as_str() {
        "hex" => b.iter().map(|x| format!("{x:02x}")).collect(),
        "base64" => base64_encode(b, false),
        "base64url" => base64_encode(b, true),
        "ascii" => b.iter().map(|&x| (x & 0x7f) as char).collect(),
        "latin1" => b.iter().map(|&x| x as char).collect(),
        "utf16le" => {
            let u: Vec<u16> = b
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u)
        }
        _ => String::from_utf8_lossy(b).into_owned(),
    }
}

pub fn string_to_bytes(s: &str, enc: &str) -> Vec<u8> {
    match norm_encoding(enc).as_str() {
        "hex" => {
            let mut out = vec![];
            let cs: Vec<char> = s.chars().collect();
            let mut i = 0;
            while i + 1 < cs.len() {
                match (cs[i].to_digit(16), cs[i + 1].to_digit(16)) {
                    (Some(a), Some(b)) => out.push((a * 16 + b) as u8),
                    _ => break,
                }
                i += 2;
            }
            out
        }
        "base64" | "base64url" => base64_decode(s),
        "ascii" | "latin1" => s.chars().map(|c| c as u32 as u8).collect(),
        "utf16le" => s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
        _ => s.as_bytes().to_vec(),
    }
}

impl<'h> Vm<'h> {
    pub fn make_buffer(&mut self, bytes: Vec<u8>) -> Value {
        let proto = self.intr.buffer_proto.clone();
        let has_proto = proto.borrow().proto.is_some();
        let t = self.new_typed(
            TypedKind::Uint8,
            bytes,
            if has_proto { Some(proto) } else { None },
        );
        Value::Obj(t)
    }
}

// ---------------------------------------------------------------- path

fn path_str(vm: &mut Vm, v: &Value, name: &str) -> JsResult<String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => {
            let d = vm.inspect_default(other)?;
            let e = vm.make_error(
                ErrKind::TypeError,
                &format!(
                    "The \"{name}\" argument must be of type string. Received {}",
                    crate::node::received(other, &d)
                ),
            );
            e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
            Err(Ctl::Throw(Value::Obj(e)))
        }
    }
}

fn p_join(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut parts = vec![];
    for v in &a.args {
        let s = path_str(vm, v, "path")?;
        if !s.is_empty() {
            parts.push(s);
        }
    }
    if parts.is_empty() {
        return Ok(Value::str("."));
    }
    let joined = parts.join("/");
    Ok(Value::string(normalize_keep_trailing(&joined)))
}

fn normalize_keep_trailing(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let trailing = p.ends_with('/');
    let mut n = normalize(p);
    if trailing && n != "/" {
        n.push('/');
    }
    n
}

fn p_normalize(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = path_str(vm, &a.arg(0), "path")?;
    Ok(Value::string(normalize_keep_trailing(&s)))
}

fn p_resolve(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut acc = String::new();
    for v in a.args.iter().rev() {
        let s = path_str(vm, v, "paths[0]")?;
        if s.is_empty() {
            continue;
        }
        acc = if acc.is_empty() {
            s.clone()
        } else {
            format!("{s}/{acc}")
        };
        if s.starts_with('/') {
            break;
        }
    }
    if !acc.starts_with('/') {
        let cwd = vm.host.cwd();
        acc = if acc.is_empty() {
            cwd
        } else {
            format!("{cwd}/{acc}")
        };
    }
    let n = normalize(&acc);
    Ok(Value::string(if n.len() > 1 {
        n.trim_end_matches('/').to_string()
    } else {
        n
    }))
}

fn p_dirname(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = path_str(vm, &a.arg(0), "path")?;
    if s.is_empty() {
        return Ok(Value::str("."));
    }
    let t = s.trim_end_matches('/');
    if t.is_empty() {
        return Ok(Value::str("/"));
    }
    Ok(Value::string(match t.rfind('/') {
        Some(0) => "/".into(),
        Some(i) => t[..i]
            .trim_end_matches('/')
            .to_string()
            .chars()
            .collect::<String>()
            .to_string(),
        None => ".".into(),
    }))
}

fn basename_of(s: &str) -> String {
    let t = s.trim_end_matches('/');
    match t.rfind('/') {
        Some(i) => t[i + 1..].to_string(),
        None => t.to_string(),
    }
}

fn p_basename(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = path_str(vm, &a.arg(0), "path")?;
    let mut b = basename_of(&s);
    if let Value::Str(ext) = a.arg(1) {
        if b.ends_with(ext.as_str()) && b != ext.as_str() {
            b.truncate(b.len() - ext.len());
        }
    }
    Ok(Value::string(b))
}

fn extname_of(s: &str) -> String {
    let b = basename_of(s);
    match b.rfind('.') {
        Some(0) | None => String::new(),
        Some(i) => b[i..].to_string(),
    }
}

fn p_extname(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = path_str(vm, &a.arg(0), "path")?;
    Ok(Value::string(extname_of(&s)))
}

fn p_is_absolute(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = path_str(vm, &a.arg(0), "path")?;
    Ok(Value::Bool(s.starts_with('/')))
}

fn p_relative(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut a1 = Args {
        this: Value::Undefined,
        args: vec![a.arg(0)],
        new_target: None,
        callee: a.callee.clone(),
    };
    let from = p_resolve(vm, &mut a1)?;
    let mut a2 = Args {
        this: Value::Undefined,
        args: vec![a.arg(1)],
        new_target: None,
        callee: a.callee.clone(),
    };
    let to = p_resolve(vm, &mut a2)?;
    let (Value::Str(f), Value::Str(t)) = (from, to) else {
        return Ok(Value::str(""));
    };
    let fp: Vec<&str> = f.split('/').filter(|x| !x.is_empty()).collect();
    let tp: Vec<&str> = t.split('/').filter(|x| !x.is_empty()).collect();
    let mut i = 0;
    while i < fp.len() && i < tp.len() && fp[i] == tp[i] {
        i += 1;
    }
    let mut out: Vec<&str> = vec![".."; fp.len() - i];
    out.extend(&tp[i..]);
    Ok(Value::string(out.join("/")))
}

fn p_parse(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = path_str(vm, &a.arg(0), "path")?;
    let root = if s.starts_with('/') { "/" } else { "" };
    let base = basename_of(&s);
    let ext = extname_of(&s);
    let name = base[..base.len() - ext.len()].to_string();
    let t = s.trim_end_matches('/');
    let dir = match t.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => t[..i].to_string(),
        None => String::new(),
    };
    let o = crate::builtins::new_obj_from(
        vm,
        vec![
            ("root", Value::str(root)),
            ("dir", Value::string(dir)),
            ("base", Value::string(base)),
            ("ext", Value::string(ext)),
            ("name", Value::string(name)),
        ],
    );
    Ok(Value::Obj(o))
}

fn p_format(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = a.arg(0);
    let get = |vm: &mut Vm, k: &str| -> JsResult<String> {
        let v = vm.get_str(&o, k)?;
        Ok(if v.is_nullish() {
            String::new()
        } else {
            vm.to_str(&v)?
        })
    };
    let dir = get(vm, "dir")?;
    let root = get(vm, "root")?;
    let base = get(vm, "base")?;
    let name = get(vm, "name")?;
    let ext = get(vm, "ext")?;
    let base = if base.is_empty() {
        format!(
            "{name}{}",
            if !ext.is_empty() && !ext.starts_with('.') {
                format!(".{ext}")
            } else {
                ext
            }
        )
    } else {
        base
    };
    let d = if dir.is_empty() {
        root.clone()
    } else {
        dir.clone()
    };
    Ok(Value::string(if d.is_empty() {
        base
    } else if d == root {
        format!("{d}{base}")
    } else {
        format!("{d}/{base}")
    }))
}

fn p_to_namespaced(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(a.arg(0))
}

pub fn make_path(vm: &mut Vm) -> Value {
    if let Some(Value::Obj(p)) = vm.global.own_value("%path") {
        return Value::Obj(p);
    }
    let p = vm.new_object();
    let fns: &[(&str, u32, NativeFn)] = &[
        ("join", 0, p_join),
        ("resolve", 0, p_resolve),
        ("normalize", 1, p_normalize),
        ("dirname", 1, p_dirname),
        ("basename", 1, p_basename),
        ("extname", 1, p_extname),
        ("isAbsolute", 1, p_is_absolute),
        ("relative", 2, p_relative),
        ("parse", 1, p_parse),
        ("format", 1, p_format),
        ("toNamespacedPath", 1, p_to_namespaced),
    ];
    for (n, l, f) in fns {
        vm.method(&p, n, *l, *f);
    }
    p.set_prop("sep", Value::str("/"), ALL);
    p.set_prop("delimiter", Value::str(":"), ALL);
    p.set_prop("posix", Value::Obj(p.clone()), ALL);
    p.set_prop("win32", Value::Obj(p.clone()), ALL);
    vm.global.set_hidden("%path", Value::Obj(p.clone()));
    Value::Obj(p)
}

// ---------------------------------------------------------------- os

pub fn make_os(vm: &mut Vm) -> Value {
    let o = vm.new_object();
    o.set_prop("EOL", Value::str("\n"), ALL);
    o.set_prop("devNull", Value::str("/dev/null"), ALL);
    let host = vm.host.hostname();
    let user = vm.host.user();
    let home = vm
        .env
        .iter()
        .find(|(k, _)| k == "HOME")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| {
            if user == "root" {
                "/root".into()
            } else {
                format!("/home/{user}")
            }
        });
    let pairs: Vec<(&str, Value)> = vec![
        ("hostname", Value::string(host)),
        ("platform", Value::str("linux")),
        ("type", Value::str("Linux")),
        ("arch", Value::str("x64")),
        ("release", Value::str("6.8.0-45-generic")),
        ("version", Value::str("#45-Ubuntu SMP PREEMPT_DYNAMIC")),
        ("machine", Value::str("x86_64")),
        ("homedir", Value::string(home.clone())),
        ("tmpdir", Value::str("/tmp")),
        ("endianness", Value::str("LE")),
        ("totalmem", Value::Num(8_336_310_272.0)),
        ("freemem", Value::Num(5_120_000_000.0)),
        ("availableParallelism", Value::Num(4.0)),
        ("uptime", Value::Num(3600.0)),
    ];
    for (k, v) in pairs {
        let f = vm.native_fn_slots(k, 0, return_slot0, vec![v]);
        o.set_prop(k, Value::Obj(f), ALL);
    }
    let cpus = vm.native_fn("cpus", 0, |vm, _a| {
        let mut out = vec![];
        for _ in 0..4 {
            let times = crate::builtins::new_obj_from(
                vm,
                vec![
                    ("user", Value::Num(100000.0)),
                    ("nice", Value::Num(0.0)),
                    ("sys", Value::Num(50000.0)),
                    ("idle", Value::Num(1000000.0)),
                    ("irq", Value::Num(0.0)),
                ],
            );
            let c = crate::builtins::new_obj_from(
                vm,
                vec![
                    ("model", Value::str("Intel(R) Xeon(R) CPU @ 2.20GHz")),
                    ("speed", Value::Num(2200.0)),
                    ("times", Value::Obj(times)),
                ],
            );
            out.push(Value::Obj(c));
        }
        Ok(vm.arr(out))
    });
    o.set_prop("cpus", Value::Obj(cpus), ALL);
    let la = vm.native_fn("loadavg", 0, |vm, _a| {
        Ok(vm.arr(vec![Value::Num(0.1), Value::Num(0.05), Value::Num(0.01)]))
    });
    o.set_prop("loadavg", Value::Obj(la), ALL);
    let ui = vm.native_fn_slots(
        "userInfo",
        0,
        |vm, a| {
            let s = slots(a);
            let o = crate::builtins::new_obj_from(
                vm,
                vec![
                    ("uid", Value::Num(1000.0)),
                    ("gid", Value::Num(1000.0)),
                    ("username", s[0].clone()),
                    ("homedir", s[1].clone()),
                    ("shell", Value::str("/bin/bash")),
                ],
            );
            Ok(Value::Obj(o))
        },
        vec![Value::string(user), Value::string(home)],
    );
    o.set_prop("userInfo", Value::Obj(ui), ALL);
    let ni = vm.native_fn("networkInterfaces", 0, |vm, _a| {
        Ok(Value::Obj(vm.new_object()))
    });
    o.set_prop("networkInterfaces", Value::Obj(ni), ALL);
    let consts = vm.new_object();
    o.set_prop("constants", Value::Obj(consts), ALL);
    Value::Obj(o)
}

fn return_slot0(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(slots(a)[0].clone())
}

// ---------------------------------------------------------------- crypto

const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

fn md5(data: &[u8]) -> Vec<u8> {
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
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
            let f2 = f.wrapping_add(a).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
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

fn block_size(name: &str) -> usize {
    if name == "sha384" || name == "sha512" {
        128
    } else {
        64
    }
}

pub fn hmac(name: &str, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let bs = block_size(name);
    let mut k = if key.len() > bs {
        digest(name, key)?
    } else {
        key.to_vec()
    };
    k.resize(bs, 0);
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(data);
    let ih = digest(name, &inner)?;
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&ih);
    digest(name, &outer)
}

fn bytes_of(vm: &mut Vm, v: &Value, enc: Option<&str>) -> JsResult<Vec<u8>> {
    match v {
        Value::Str(s) => Ok(string_to_bytes(s, enc.unwrap_or("utf8"))),
        Value::Obj(o) => Ok(vm.typed_bytes(o).unwrap_or_default()),
        other => {
            let s = vm.to_str(other)?;
            Ok(s.into_bytes())
        }
    }
}

/// binding.digest(alg, dataParts[], encoding, hmacKey?)
fn b_digest(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let alg = str_arg(vm, a, 0)?.to_ascii_lowercase();
    let parts = vm.iterable_to_vec(&a.arg(1))?;
    let mut data = vec![];
    for p in parts {
        data.extend(bytes_of(vm, &p, None)?);
    }
    let key = a.arg(3);
    let out = if key.is_undefined() {
        digest(&alg, &data)
    } else {
        let k = bytes_of(vm, &key, None)?;
        hmac(&alg, &k, &data)
    };
    let Some(out) = out else {
        let e = vm.make_error(ErrKind::Error, "Digest method not supported");
        e.set_prop("code", Value::str("ERR_OSSL_EVP_UNSUPPORTED"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    };
    match a.arg(2) {
        Value::Str(enc) => Ok(Value::string(bytes_to_string(&out, &enc))),
        _ => Ok(vm.make_buffer(out)),
    }
}

fn random_bytes_vec(vm: &mut Vm, n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let r = vm.host.random_u64();
        for b in r.to_le_bytes() {
            if out.len() < n {
                out.push(b);
            }
        }
    }
    out
}

fn c_random_bytes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = vm.to_integer(&a.arg(0))?.max(0.0) as usize;
    let b = random_bytes_vec(vm, n);
    let buf = vm.make_buffer(b);
    if a.arg(1).is_callable() {
        vm.ticks
            .push_back((a.arg(1), vec![Value::Null, buf.clone()]));
        return Ok(Value::Undefined);
    }
    Ok(buf)
}

fn c_random_uuid(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let mut b = random_bytes_vec(vm, 16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(Value::string(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )))
}

fn c_random_int(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (min, max) = if a.args.len() >= 2 && !a.arg(1).is_callable() {
        (vm.to_integer(&a.arg(0))?, vm.to_integer(&a.arg(1))?)
    } else {
        (0.0, vm.to_integer(&a.arg(0))?)
    };
    if max <= min {
        let e = vm.make_error(ErrKind::RangeError, &format!("The value of \"max\" is out of range. It must be greater than the value of \"min\" ({}). Received {}", min, max));
        e.set_prop("code", Value::str("ERR_OUT_OF_RANGE"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    }
    let span = (max - min) as u64;
    let r = vm.host.random_u64() % span;
    Ok(Value::Num(min + r as f64))
}

fn c_get_random_values(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = a.arg(0) {
        let (buf, off, len, sz) = match &o.borrow().kind {
            Kind::TypedArray {
                buf,
                offset,
                len,
                kind,
                ..
            } => (
                buf.clone(),
                *offset,
                *len,
                crate::builtins::typed::elem_size(*kind),
            ),
            _ => return Ok(a.arg(0)),
        };
        let bytes = random_bytes_vec(vm, len * sz);
        buf.borrow_mut()[off..off + len * sz].copy_from_slice(&bytes);
    }
    Ok(a.arg(0))
}

fn c_timing_safe_equal(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let x = bytes_of(vm, &a.arg(0), None)?;
    let y = bytes_of(vm, &a.arg(1), None)?;
    Ok(Value::Bool(x == y))
}

fn c_get_hashes(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let v: Vec<Value> = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"]
        .iter()
        .map(|s| Value::str(s))
        .collect();
    Ok(vm.arr(v))
}

pub fn make_crypto(vm: &mut Vm) -> Value {
    let src = include_str!("../js/crypto.js");
    match vm.load_internal_js("node:crypto", src, "crypto") {
        Ok(v) => v,
        Err(_) => Value::Obj(vm.new_object()),
    }
}

// ---------------------------------------------------------------- util

pub fn util_inspect(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let opts_v = a.arg(1);
    let o = if let Value::Obj(_) = &opts_v {
        vm.inspect_opts_from(&opts_v)?
    } else {
        // Legacy (obj, showHidden, depth, colors).
        let mut o = crate::inspect::Opts::default();
        if let Value::Bool(b) = opts_v {
            o.show_hidden = b;
        }
        match a.arg(2) {
            Value::Null => o.depth = None,
            Value::Num(n) => o.depth = Some(n),
            _ => {}
        }
        o
    };
    let s = vm.inspect(&a.arg(0), &o)?;
    Ok(Value::string(s))
}

fn b_format(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = vm.format_args(&a.args)?;
    Ok(Value::string(s))
}

fn b_type_tag(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Ok(Value::str(""));
    };
    let d = o.borrow();
    Ok(Value::str(match &d.kind {
        Kind::Array(_) => "Array",
        Kind::Function(fd) => match &fd.imp {
            FuncImpl::Closure { code, .. } => {
                if code.is_async && code.is_generator {
                    "AsyncGeneratorFunction"
                } else if code.is_async {
                    "AsyncFunction"
                } else if code.is_generator {
                    "GeneratorFunction"
                } else {
                    "Function"
                }
            }
            _ => "Function",
        },
        Kind::Error(_) => "Error",
        Kind::Boolean(_)
        | Kind::Number(_)
        | Kind::String(_)
        | Kind::Symbol(_)
        | Kind::BigInt(_) => "Boxed",
        Kind::Date(_) => "Date",
        Kind::RegExp(_) => "RegExp",
        Kind::Map(_) => "Map",
        Kind::Set(_) => "Set",
        Kind::WeakMap(_) => "WeakMap",
        Kind::WeakSet(_) => "WeakSet",
        Kind::Promise(_) => "Promise",
        Kind::Generator(gd) => {
            if gd.is_async {
                "AsyncGenerator"
            } else {
                "Generator"
            }
        }
        Kind::MapIter { target, .. } => {
            if matches!(target.borrow().kind, Kind::Set(_)) {
                "SetIterator"
            } else {
                "MapIterator"
            }
        }
        Kind::Arguments => "Arguments",
        Kind::ArrayBuffer(_) => "ArrayBuffer",
        Kind::TypedArray { .. } => "TypedArray",
        Kind::Proxy { .. } => "Proxy",
        _ => "Object",
    }))
}

fn b_set_buffer_proto(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(p) = a.arg(0) {
        vm.intr.buffer_proto = p;
    }
    Ok(Value::Undefined)
}

/// bufferFrom(string, encoding) -> Buffer
fn b_buffer_from_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let enc = match a.arg(1) {
        Value::Str(e) => e.to_string(),
        _ => "utf8".into(),
    };
    let e = norm_encoding(&enc);
    if !matches!(
        e.as_str(),
        "utf8" | "hex" | "base64" | "base64url" | "ascii" | "latin1" | "utf16le"
    ) {
        let err = vm.make_error(ErrKind::TypeError, &format!("Unknown encoding: {enc}"));
        err.set_prop("code", Value::str("ERR_UNKNOWN_ENCODING"), ALL);
        return Err(Ctl::Throw(Value::Obj(err)));
    }
    Ok(vm.make_buffer(string_to_bytes(&s, &e)))
}

/// bufferToString(buf, encoding, start, end)
fn b_buffer_to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Ok(Value::str(""));
    };
    let bytes = vm.typed_bytes(&o).unwrap_or_default();
    let enc = match a.arg(1) {
        Value::Str(e) => e.to_string(),
        _ => "utf8".into(),
    };
    let len = bytes.len();
    let s = if a.arg(2).is_undefined() {
        0
    } else {
        (vm.to_integer(&a.arg(2))?.max(0.0) as usize).min(len)
    };
    let e = if a.arg(3).is_undefined() {
        len
    } else {
        (vm.to_integer(&a.arg(3))?.max(0.0) as usize).min(len)
    };
    let slice = if s < e { &bytes[s..e] } else { &[][..] };
    Ok(Value::string(bytes_to_string(slice, &enc)))
}

fn b_byte_length(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = a.arg(0);
    let enc = match a.arg(1) {
        Value::Str(e) => e.to_string(),
        _ => "utf8".into(),
    };
    let n = match &v {
        Value::Str(s) => string_to_bytes(s, &enc).len(),
        Value::Obj(o) => vm.typed_bytes(o).map(|b| b.len()).unwrap_or(0),
        _ => 0,
    };
    Ok(Value::Num(n as f64))
}

fn b_read_stdin(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    crate::node::stdin_rest(vm)
}

fn b_promise_state(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Ok(Value::Undefined);
    };
    match vm.promise_state(&o) {
        Some((st, v)) => {
            let s = match st {
                PromiseState::Pending => "pending",
                PromiseState::Fulfilled => "fulfilled",
                PromiseState::Rejected => "rejected",
            };
            Ok(vm.arr(vec![Value::str(s), v]))
        }
        None => Ok(Value::Undefined),
    }
}

fn b_set_error_frames(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    // setErrorArrow(err, arrow, dropFrames): internal modules adjust headers.
    if let Value::Obj(o) = a.arg(0) {
        let arrow = match a.arg(1) {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        };
        let drop_n = match a.arg(2) {
            Value::Num(n) => n as usize,
            _ => 0,
        };
        let prepend: Vec<String> = match a.arg(3) {
            Value::Obj(arr) if arr.is_array() => {
                let items = vm.iterable_to_vec(&Value::Obj(arr))?;
                let mut v = vec![];
                for it in items {
                    v.push(vm.to_str(&it)?);
                }
                v
            }
            _ => vec![],
        };
        let changed = drop_n > 0 || !prepend.is_empty();
        if let Kind::Error(ed) = &mut o.borrow_mut().kind {
            if arrow.is_some() {
                ed.arrow = arrow;
            }
            let n = drop_n.min(ed.frames.len());
            ed.frames.drain(..n);
            for (i, p) in prepend.into_iter().enumerate() {
                ed.frames.insert(i, p);
            }
        }
        if changed {
            // Re-format lazily.
            o.borrow_mut()
                .props
                .insert(Key::str("stack"), Prop::data(Value::Empty, HIDDEN));
        }
    }
    Ok(Value::Undefined)
}

fn b_exit_code(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(vm.exit_code as f64))
}

fn b_has_stdin(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(
        vm.stdin.as_deref().map(|s| !s.is_empty()).unwrap_or(false) && !vm.stdin_consumed,
    ))
}

fn b_is_tty(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(vm.interactive))
}

fn b_callsite_name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    // Source text of a caller's line (for assert's "falsy value" message).
    let depth = match a.arg(0) {
        Value::Num(d) => d as usize,
        _ => 0,
    };
    let n = vm.frames.len();
    if n < depth + 1 {
        return Ok(Value::Undefined);
    }
    let f = &vm.frames[n - 1 - depth];
    let pos = f
        .code
        .pos
        .get(f.pc.saturating_sub(1))
        .copied()
        .unwrap_or_default();
    let file = f.code.file.clone();
    let Some(src) = vm.source_for(&file) else {
        return Ok(Value::Undefined);
    };
    let line = src
        .split('\n')
        .nth(pos.line.saturating_sub(1) as usize)
        .unwrap_or("")
        .to_string();
    let o = crate::builtins::new_obj_from(
        vm,
        vec![
            ("line", Value::string(line)),
            ("col", Value::Num(pos.col as f64)),
        ],
    );
    Ok(Value::Obj(o))
}

pub fn make_binding(vm: &mut Vm) -> Obj {
    let b = vm.new_object();
    let fns: &[(&str, u32, NativeFn)] = &[
        ("format", 0, b_format),
        ("inspect", 2, util_inspect),
        ("typeTag", 1, b_type_tag),
        ("setBufferProto", 1, b_set_buffer_proto),
        ("bufferFromString", 2, b_buffer_from_string),
        ("bufferToString", 4, b_buffer_to_string),
        ("byteLength", 2, b_byte_length),
        ("readStdin", 0, b_read_stdin),
        ("hasStdin", 0, b_has_stdin),
        ("isTTY", 1, b_is_tty),
        ("promiseState", 1, b_promise_state),
        ("setErrorFrames", 4, b_set_error_frames),
        ("exitCode", 0, b_exit_code),
        ("digest", 4, b_digest),
        ("randomBytes", 2, c_random_bytes),
        ("randomUUID", 0, c_random_uuid),
        ("randomInt", 2, c_random_int),
        ("getRandomValues", 1, c_get_random_values),
        ("timingSafeEqual", 2, c_timing_safe_equal),
        ("getHashes", 0, c_get_hashes),
        ("callsite", 0, b_callsite_name),
    ];
    for (n, l, f) in fns {
        vm.method(&b, n, *l, *f);
    }
    let _ = dirname;
    crate::hostio::install(vm, &b);
    crate::workers::install(vm, &b);
    b
}

// ---------------------------------------------------------------- Stats / Dirent

fn stat_is(vm: &mut Vm, a: &Args, mask: u32) -> JsResult<Value> {
    let m = vm.get_str(&a.this, "mode")?;
    let mode = vm.to_number(&m)? as u32;
    Ok(Value::Bool(mode & 0o170000 == mask))
}

pub fn stats_proto(vm: &mut Vm) -> Obj {
    if let Some(Value::Obj(p)) = vm.global.own_value("%StatsProto") {
        return p;
    }
    let p = vm.new_object();
    let ctor = vm.native_fn("Stats", 0, |vm, _a| Ok(Value::Obj(vm.new_object())));
    ctor.set_prop("prototype", Value::Obj(p.clone()), 0);
    p.set_hidden("constructor", Value::Obj(ctor));
    vm.method(&p, "isFile", 0, |vm, a| stat_is(vm, a, 0o100000));
    vm.method(&p, "isDirectory", 0, |vm, a| stat_is(vm, a, 0o040000));
    vm.method(&p, "isSymbolicLink", 0, |vm, a| stat_is(vm, a, 0o120000));
    vm.method(&p, "isFIFO", 0, |_vm, _a| Ok(Value::Bool(false)));
    vm.method(&p, "isSocket", 0, |_vm, _a| Ok(Value::Bool(false)));
    vm.method(&p, "isBlockDevice", 0, |_vm, _a| Ok(Value::Bool(false)));
    vm.method(&p, "isCharacterDevice", 0, |_vm, _a| Ok(Value::Bool(false)));
    // Date fields are materialised on first access, like Node.
    for k in ["atime", "mtime", "ctime", "birthtime"] {
        let g = vm.native_fn_slots(
            &format!("get {k}"),
            0,
            stat_date_getter,
            vec![Value::str(k)],
        );
        p.borrow_mut().props.insert(
            Key::str(k),
            Prop {
                slot: Slot::Accessor(Some(g), None),
                flags: CONFIGURABLE,
            },
        );
    }
    vm.global.set_hidden("%StatsProto", Value::Obj(p.clone()));
    p
}

fn stat_date_getter(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let name = match &slots(a)[0] {
        Value::Str(s) => s.to_string(),
        _ => return Ok(Value::Undefined),
    };
    let ms = vm.get_str(&a.this, &format!("{name}Ms"))?;
    let t = vm.to_number(&ms)?;
    let d = Value::Obj(vm.obj_with(Some(vm.intr.date_proto.clone()), Kind::Date(t.floor())));
    if let Value::Obj(o) = &a.this {
        o.set_prop(&name, d.clone(), ALL);
    }
    Ok(d)
}

fn dirent_is(vm: &mut Vm, a: &Args, t: f64) -> JsResult<Value> {
    let sym = crate::fs::dirent_type_symbol(vm);
    Ok(Value::Bool(match &a.this {
        Value::Obj(o) => {
            matches!(o.borrow().props.get(&Key::Sym(sym)), Some(Prop { slot: Slot::Data(Value::Num(x)), .. }) if *x == t)
        }
        _ => false,
    }))
}

pub fn dirent_proto(vm: &mut Vm) -> Obj {
    if let Some(Value::Obj(p)) = vm.global.own_value("%DirentProto") {
        return p;
    }
    let p = vm.new_object();
    let ctor = vm.native_fn("Dirent", 0, |vm, _a| Ok(Value::Obj(vm.new_object())));
    ctor.set_prop("prototype", Value::Obj(p.clone()), 0);
    p.set_hidden("constructor", Value::Obj(ctor));
    vm.method(&p, "isFile", 0, |vm, a| dirent_is(vm, a, 1.0));
    vm.method(&p, "isDirectory", 0, |vm, a| dirent_is(vm, a, 2.0));
    vm.method(&p, "isSymbolicLink", 0, |vm, a| dirent_is(vm, a, 3.0));
    vm.method(&p, "isFIFO", 0, |_vm, _a| Ok(Value::Bool(false)));
    vm.method(&p, "isSocket", 0, |_vm, _a| Ok(Value::Bool(false)));
    vm.global.set_hidden("%DirentProto", Value::Obj(p.clone()));
    let _ = Rc::new(0);
    p
}
