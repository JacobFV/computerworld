//! `python3` and `node` reach the reference world's services through the world's
//! own network: DNS, routes, listeners and the TLS model the browser sees.
use computerworld::{reference_world, World};

fn world() -> World {
    World::new(reference_world(), 5).unwrap()
}
fn run(world: &mut World, file: &str, src: &str, command: &str) -> (String, String, i32) {
    let rt = world.runtime_mut();
    rt.write_file("alice-mac", "alice", file, src.as_bytes())
        .unwrap();
    let r = rt.execute("alice-mac", "alice", command).unwrap();
    (r.stdout, r.stderr, r.exit_code)
}

const PY: &str = r#"
import socket, re, urllib.request, urllib.error, http.client
print(socket.gethostbyname('intranet.internal'))
print(socket.getaddrinfo('intranet.internal', 80, type=socket.SOCK_STREAM)[0][4])
with urllib.request.urlopen('http://intranet.internal/') as r:
    body = r.read()
    print(r.status, r.headers.get_content_type(), re.search(r'<title>(.*?)</title>', body.decode()).group(1))
try:
    urllib.request.urlopen('http://intranet.internal/definitely-missing')
except urllib.error.HTTPError as e:
    print('HTTPError', e.code)
with urllib.request.urlopen('https://intranet.internal/') as r:
    print('https', r.status)
try:
    urllib.request.urlopen('http://nowhere.invalid/')
except urllib.error.URLError as e:
    print('URLError', e.reason)
c = http.client.HTTPConnection('intranet.internal', 80, timeout=5)
c.request('GET', '/')
r = c.getresponse()
print('http.client', r.status, len(r.read()) > 100)
s = socket.create_connection(('intranet.internal', 80))
s.sendall(b'GET / HTTP/1.1\r\nHost: intranet.internal\r\nConnection: close\r\n\r\n')
data = b''
while True:
    chunk = s.recv(4096)
    if not chunk:
        break
    data += chunk
print(data.split(b'\r\n')[0].decode())
try:
    socket.create_connection(('intranet.internal', 8081))
except ConnectionRefusedError as e:
    print('refused', e.errno)
"#;

const JS: &str = r#"
const http = require('http');
const dns = require('dns');
dns.lookup('intranet.internal', (err, address) => console.log('lookup', err, address));
http.get('http://intranet.internal/', (res) => {
  let n = 0;
  res.on('data', (c) => (n += c.length));
  res.on('end', () => console.log('http', res.statusCode, n > 100));
});
fetch('http://intranet.internal/').then(async (r) => console.log('fetch', r.status, (await r.text()).match(/<title>(.*?)<\/title>/)[1]));
fetch('https://intranet.internal/').then((r) => console.log('https', r.status));
fetch('http://nowhere.invalid/').catch((e) => console.log('dns', e.cause.message));
const net = require('net');
const s = net.connect(80, 'intranet.internal', () => s.end('GET / HTTP/1.1\r\nHost: intranet.internal\r\nConnection: close\r\n\r\n'));
let raw = '';
s.on('data', (d) => (raw += d));
s.on('close', () => console.log('net', raw.split('\r\n')[0]));
"#;

#[test]
fn python_reaches_world_services() {
    let mut w = world();
    let (out, err, code) = run(&mut w, "/tmp/net.py", PY, "python3 /tmp/net.py");
    assert_eq!(err, "");
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("10.0."), "{out}");
    assert_eq!(lines[1], format!("('{}', 80)", lines[0]));
    assert_eq!(
        lines[2],
        "200 text/html Northstar Workshop"
    );
    assert_eq!(lines[3], "HTTPError 404");
    // Every site listens on 443 as well, so https reaches the same page.
    assert_eq!(lines[4], "https 200");
    assert_eq!(lines[5], "URLError [Errno -2] Name or service not known");
    assert_eq!(lines[6], "http.client 200 True");
    assert_eq!(lines[7], "HTTP/1.1 200 OK");
    assert_eq!(lines[8], "refused 111");
    // Same seed, same world: the same bytes.
    let mut again = world();
    assert_eq!(
        run(&mut again, "/tmp/net.py", PY, "python3 /tmp/net.py").0,
        out
    );
}

#[test]
fn node_reaches_world_services() {
    let mut w = world();
    let (out, err, code) = run(&mut w, "/tmp/net.js", JS, "node /tmp/net.js");
    assert_eq!(err, "");
    assert_eq!(code, 0);
    assert!(out.contains("lookup null 10.0."), "{out}");
    assert!(out.contains("http 200 true\n"), "{out}");
    assert!(out.contains("fetch 200 Northstar Workshop\n"), "{out}");
    assert!(out.contains("https 200\n"), "{out}");
    assert!(
        out.contains("dns getaddrinfo ENOTFOUND nowhere.invalid\n"),
        "{out}"
    );
    assert!(out.contains("net HTTP/1.1 200 OK\n"), "{out}");
    let mut again = world();
    assert_eq!(
        run(&mut again, "/tmp/net.js", JS, "node /tmp/net.js").0,
        out
    );
}

#[test]
fn a_stopped_service_refuses_runtimes_like_any_client() {
    let mut w = world();
    let rt = w.runtime_mut();
    let r = rt
        .execute("app-server", "admin", "systemctl stop intranet")
        .unwrap();
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let (out, _, _) = run(
        &mut w,
        "/tmp/s.py",
        "import urllib.request\ntry:\n    urllib.request.urlopen('http://intranet.internal/')\nexcept OSError as e:\n    print(type(e).__name__, e)\n",
        "python3 /tmp/s.py",
    );
    assert_eq!(
        out,
        "URLError <urlopen error [Errno 111] Connection refused>\n"
    );
}
