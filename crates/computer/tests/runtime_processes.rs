//! Child processes from inside `python3` and `node`: they run through this machine's
//! own shell (builtins, scripts, nested interpreters), deterministically.
use cw_computer::{shell, CommandResult, Computer, OfflineHost};

fn machine() -> Computer {
    let mut c = Computer::new("box", "user", "linux", true);
    for setup in [
        "mkdir -p /home/user/proj",
        "printf 'alpha\\nbeta\\ngamma\\n' > /home/user/proj/a.txt",
    ] {
        let r = run(&mut c, setup);
        assert_eq!(r.exit_code, 0, "{setup}: {}", r.stderr);
    }
    c
}
fn run(c: &mut Computer, line: &str) -> CommandResult {
    shell::execute(c, line, 0, &mut OfflineHost)
}
fn write(c: &mut Computer, path: &str, text: &str) {
    c.vfs.write_as(path, text.as_bytes(), "user", 0).unwrap();
}
fn py(c: &mut Computer, src: &str) -> CommandResult {
    write(c, "/home/user/t.py", src);
    run(c, "cd /home/user && python3 t.py")
}
fn js(c: &mut Computer, src: &str) -> CommandResult {
    write(c, "/home/user/t.js", src);
    run(c, "cd /home/user && node t.js")
}

#[test]
fn python_subprocess_runs_machine_commands() {
    let mut c = machine();
    let r = py(
        &mut c,
        r#"import subprocess, os
r = subprocess.run(['ls', 'proj'], capture_output=True, text=True)
print(repr(r.stdout), r.returncode)
r = subprocess.run('cat proj/a.txt | wc -l', shell=True, capture_output=True, text=True)
print(r.stdout.strip())
print(subprocess.check_output(['echo', 'hi']))
r = subprocess.run(['python3', '-c', 'import sys; print(sys.stdin.read().upper()); sys.exit(3)'],
                   input='piped', capture_output=True, text=True)
print(repr(r.stdout), r.returncode)
try:
    subprocess.run(['false'], check=True)
except subprocess.CalledProcessError as e:
    print('CalledProcessError', e)
try:
    subprocess.run(['no-such-program'])
except FileNotFoundError as e:
    print('FileNotFoundError', e)
r = subprocess.run(['pwd'], cwd='/tmp', capture_output=True, text=True)
print(r.stdout.strip())
r = subprocess.run('echo $GREETING', shell=True, capture_output=True, text=True,
                   env={'GREETING': 'hello', 'PATH': '/bin:/usr/bin'})
print(r.stdout.strip())
os.environ['FROM_PARENT'] = 'yes'
print(subprocess.getoutput('echo $FROM_PARENT'))
print(subprocess.getstatusoutput('exit 4'))
p = subprocess.Popen(['cat'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
out, err = p.communicate('via popen\n')
print(repr(out), p.returncode)
print(os.system('echo from system') >> 8)
with os.popen('echo popen') as f:
    print(f.read().strip())
r = subprocess.run(['node', '-e', 'console.log(6*7)'], capture_output=True, text=True)
print(r.stdout.strip())
r = subprocess.run(['python3', '-c', 'import time; time.sleep(3)'], capture_output=True)
try:
    subprocess.run(['python3', '-c', 'import time; time.sleep(5)'], timeout=1)
except subprocess.TimeoutExpired as e:
    print('TimeoutExpired', e)
"#,
    );
    assert_eq!(r.stderr, "");
    // `os.system` writes straight to the descriptor while our own prints are still
    // in CPython's pipe buffer, so its line comes first.
    assert_eq!(
        r.stdout,
        "from system\n'a.txt\\n' 0\n3\nb'hi\\n'\n'PIPED\\n' 3\nCalledProcessError Command '['false']' returned non-zero exit status 1.\nFileNotFoundError [Errno 2] No such file or directory: 'no-such-program'\n/tmp\nhello\nyes\n(4, '')\n'via popen\\n' 0\n0\npopen\n42\nTimeoutExpired Command '['python3', '-c', 'import time; time.sleep(5)']' timed out after 1 seconds\n"
    );
}

#[test]
fn python_child_output_lands_after_flushed_output() {
    let mut c = machine();
    // Piped stdout is block-buffered in CPython: the child's line comes first
    // unless the parent flushed.
    let r = py(
        &mut c,
        "import subprocess, sys\nprint('parent')\nsubprocess.run(['echo', 'child'])\nprint('flushed', flush=True)\nsubprocess.run(['echo', 'child2'])\n",
    );
    assert_eq!(r.stdout, "child\nparent\nflushed\nchild2\n");
}

#[test]
fn gzip_commands_and_runtimes_share_the_format() {
    let mut c = machine();
    let r = run(&mut c, "cd /home/user/proj && gzip -k a.txt && ls");
    assert_eq!(r.stdout, "a.txt\na.txt.gz\n", "{}", r.stderr);
    assert_eq!(
        run(&mut c, "zcat /home/user/proj/a.txt.gz").stdout,
        "alpha\nbeta\ngamma\n"
    );
    // Python and Node read what gzip wrote, and write what gunzip reads.
    let r = py(
        &mut c,
        "import gzip\nprint(gzip.open('proj/a.txt.gz').read())\nwith gzip.open('proj/py.gz', 'wt') as f:\n    f.write('from python\\n')\n",
    );
    assert_eq!(r.stdout, "b'alpha\\nbeta\\ngamma\\n'\n", "{}", r.stderr);
    assert_eq!(
        run(&mut c, "zcat /home/user/proj/py.gz").stdout,
        "from python\n"
    );
    let r = js(
        &mut c,
        "const fs = require('fs'), zlib = require('zlib');\nconsole.log(zlib.gunzipSync(fs.readFileSync('proj/a.txt.gz')).toString().trim());\nfs.writeFileSync('proj/js.gz', zlib.gzipSync('from node\\n'));\n",
    );
    assert_eq!(r.stdout, "alpha\nbeta\ngamma\n", "{}", r.stderr);
    let r = run(&mut c, "cd /home/user/proj && gunzip js.gz && cat js");
    assert_eq!(r.stdout, "from node\n", "{}", r.stderr);
    let r = run(&mut c, "gzip -c /home/user/proj/a.txt");
    assert_eq!(r.exit_code, 1);
    assert!(r
        .stderr
        .contains("compressed data not written to a terminal"));
    let r = run(&mut c, "cd /home/user/proj && gzip a.txt");
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("a.txt.gz already exists"), "{}", r.stderr);
}

#[test]
fn node_child_process_runs_machine_commands() {
    let mut c = machine();
    let r = js(
        &mut c,
        r#"const cp = require('child_process');
console.log(JSON.stringify(cp.execSync('ls proj').toString()));
console.log(cp.execSync('cat proj/a.txt | wc -l', { encoding: 'utf8' }).trim());
const r = cp.spawnSync('python3', ['-c', 'import sys; print(sys.stdin.read()[::-1]); sys.exit(2)'], { input: 'abc', encoding: 'utf8' });
console.log(JSON.stringify(r.stdout), r.status, r.signal);
try { cp.execSync('exit 5', { stdio: 'pipe' }); } catch (e) { console.log('status', e.status, e.message.split('\n')[0]); }
try { cp.execFileSync('no-such-program'); } catch (e) { console.log(e.code, e.syscall); }
console.log(cp.execFileSync('pwd', { cwd: '/tmp', encoding: 'utf8' }).trim());
console.log(cp.execSync('echo $GREETING', { env: { GREETING: 'hi', PATH: '/bin:/usr/bin' }, encoding: 'utf8' }).trim());
cp.exec('echo async', (err, stdout, stderr) => console.log('exec', err, JSON.stringify(stdout)));
const child = cp.spawn('sh', ['-c', 'cat; echo done >&2']);
let out = '';
child.stdout.on('data', d => out += d);
child.stderr.on('data', d => out += '[err]' + d);
child.on('close', (code) => console.log('close', code, JSON.stringify(out)));
child.stdin.write('to child\n');
child.stdin.end();
cp.execFile('node', ['-e', 'process.exit(9)'], (err) => console.log('execFile', err.code));
"#,
    );
    assert_eq!(r.stderr, "");
    assert_eq!(
        r.stdout,
        "\"a.txt\\n\"\n3\n\"cba\\n\" 2 null\nstatus 5 Command failed: exit 5\nENOENT spawnSync no-such-program\n/tmp\nhi\nexec null \"async\\n\"\nclose 0 \"to child\\n[err]done\\n\"\nexecFile 9\n"
    );
}
