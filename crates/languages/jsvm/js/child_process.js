'use strict';
// Child processes run through the machine's own shell (builtins, scripts, nested
// node and python3). A child runs to completion when it starts; asynchronous
// forms deliver its output and exit as I/O completions at the simulated time the
// child took.

const EventEmitter = require('events');
const { Readable, Writable } = require('stream');

function envPairs(env) {
  const src = env === undefined ? process.env : env;
  const out = [];
  for (const k of Object.keys(src)) {
    if (src[k] !== undefined) out.push(String(k), String(src[k]));
  }
  return out;
}

function normalizeArgs(file, args, options) {
  if (Array.isArray(args)) return [file, args.map(String), options || {}];
  if (args != null && typeof args === 'object') return [file, [], args];
  return [file, [], options || {}];
}

function shellFor(options) {
  return options && options.shell;
}

function run(file, args, options, input) {
  const shell = shellFor(options);
  let command = null;
  if (shell) command = [file, ...args].join(' ');
  const cwd = options.cwd === undefined ? undefined : String(options.cwd);
  let data = input;
  if (data === undefined) data = options.input;
  return binding.spawn(file, args, command, data, cwd, envPairs(options.env));
}

function spawnError(syscall, file, args, r) {
  const e = new Error(`${syscall} ${file} ${r.error}`);
  e.errno = r.errno;
  e.code = r.error;
  e.syscall = `${syscall} ${file}`;
  e.path = file;
  e.spawnargs = args;
  return e;
}

function decode(buf, encoding) {
  if (encoding && encoding !== 'buffer') return buf.toString(encoding);
  return buf;
}

function stdioOf(options, i) {
  const s = options.stdio;
  if (s === undefined) return 'pipe';
  if (typeof s === 'string') return s;
  return s[i] === undefined || s[i] === null ? 'pipe' : s[i];
}

function spawnSync(file, args, options) {
  [file, args, options] = normalizeArgs(file, args, options);
  const r = run(file, args, options);
  if (r.error) {
    const err = spawnError('spawnSync', file, args, r);
    return { error: err, status: null, signal: null, output: null, pid: 0, stdout: null, stderr: null };
  }
  binding.advance(r.elapsedMs);
  const enc = options.encoding;
  let stdout = decode(r.stdout, enc);
  let stderr = decode(r.stderr, enc);
  if (stdioOf(options, 1) === 'inherit') { binding.childOutput(r.stdout, ''); stdout = null; }
  if (stdioOf(options, 2) === 'inherit') { binding.childOutput('', r.stderr); stderr = null; }
  return { status: r.status, signal: null, output: [null, stdout, stderr], pid: process.pid + 1, stdout, stderr };
}

function checkExecSyncError(ret, args, cmd) {
  let err;
  if (ret.error) {
    err = ret.error;
    Object.assign(err, ret);
    delete err.error;
  } else if (ret.status !== 0) {
    let msg = 'Command failed: ';
    msg += cmd || args.join(' ');
    if (ret.stderr && ret.stderr.length > 0) msg += `\n${ret.stderr.toString()}`;
    err = new Error(msg);
    err.status = ret.status;
    err.signal = ret.signal;
    err.output = ret.output;
    err.pid = ret.pid;
    err.stdout = ret.stdout;
    err.stderr = ret.stderr;
  }
  return err;
}

function execFileSync(file, args, options) {
  [file, args, options] = normalizeArgs(file, args, options);
  const inheritErr = options.stdio === undefined;
  const ret = spawnSync(file, args, { ...options, stdio: options.stdio });
  if (inheritErr && ret.stderr && ret.stderr.length) binding.childOutput('', ret.stderr);
  const err = checkExecSyncError(ret, [file, ...args], undefined);
  if (err) throw err;
  return ret.stdout;
}

function execSync(command, options) {
  options = options || {};
  const inheritErr = options.stdio === undefined;
  const ret = spawnSync(String(command), [], { ...options, shell: true });
  if (inheritErr && ret.stderr && ret.stderr.length) binding.childOutput('', ret.stderr);
  const err = checkExecSyncError(ret, [], String(command));
  if (err) throw err;
  return ret.stdout;
}

class ChildProcess extends EventEmitter {
  constructor() {
    super();
    this.pid = undefined;
    this.exitCode = null;
    this.signalCode = null;
    this.killed = false;
    this.connected = false;
    this.stdin = null;
    this.stdout = null;
    this.stderr = null;
    this.stdio = [null, null, null];
    this.spawnfile = undefined;
    this.spawnargs = [];
  }
  kill(signal) { this.killed = true; return false; }
  ref() {}
  unref() {}
  disconnect() {}
  send() { return false; }
}

function spawn(file, args, options) {
  [file, args, options] = normalizeArgs(file, args, options);
  const child = new ChildProcess();
  child.spawnfile = options.shell ? '/bin/sh' : file;
  child.spawnargs = options.shell ? ['/bin/sh', '-c', [file, ...args].join(' ')] : [file, ...args];
  const chunks = [];
  let ended = false;
  let started = false;
  const pipeIn = stdioOf(options, 0) === 'pipe';
  const pipeOut = stdioOf(options, 1) === 'pipe';
  const pipeErr = stdioOf(options, 2) === 'pipe';
  if (pipeIn) {
    child.stdin = new Writable({
      write(chunk, enc, cb) { chunks.push(typeof chunk === 'string' ? Buffer.from(chunk, enc) : chunk); cb(); },
      final(cb) { ended = true; setImmediate(start); cb(); },
    });
  }
  if (pipeOut) child.stdout = new Readable({});
  if (pipeErr) child.stderr = new Readable({});
  child.stdio = [child.stdin, child.stdout, child.stderr];
  const start = () => {
    if (started) return;
    started = true;
    const r = run(file, args, options, Buffer.concat(chunks));
    if (r.error) {
      const err = spawnError('spawn', file, args, r);
      child.emit('error', err);
      child.emit('close', -2, null);
      return;
    }
    child.pid = process.pid + 1;
    const finish = () => {
      if (pipeOut) { if (r.stdout.length) child.stdout.push(options.encoding ? r.stdout.toString(options.encoding) : r.stdout); child.stdout.push(null); }
      else if (stdioOf(options, 1) === 'inherit') binding.childOutput(r.stdout, '');
      if (pipeErr) { if (r.stderr.length) child.stderr.push(options.encoding ? r.stderr.toString(options.encoding) : r.stderr); child.stderr.push(null); }
      else if (stdioOf(options, 2) === 'inherit') binding.childOutput('', r.stderr);
      child.exitCode = r.status;
      setImmediate(() => {
        child.emit('exit', r.status, null);
        setImmediate(() => child.emit('close', r.status, null));
      });
    };
    binding.scheduleIo(finish, r.elapsedMs);
  };
  process.nextTick(() => {
    child.emit('spawn');
    // A child whose input nobody writes starts right away with empty input.
    setImmediate(() => { if (!pipeIn || (chunks.length === 0 && !ended)) { if (child.stdin) child.stdin.writable = false; start(); } });
  });
  return child;
}

function execFile(file, args, options, callback) {
  if (typeof args === 'function') { callback = args; args = []; options = {}; }
  else if (typeof options === 'function') { callback = options; options = Array.isArray(args) ? {} : args; if (!Array.isArray(args)) args = []; }
  [file, args, options] = normalizeArgs(file, args, options);
  const encoding = options.encoding === undefined ? 'utf8' : options.encoding;
  const child = new ChildProcess();
  child.spawnfile = file;
  child.spawnargs = [file, ...args];
  child.stdout = new Readable({});
  child.stderr = new Readable({});
  child.stdin = new Writable({});
  const cmd = [file, ...args].join(' ');
  process.nextTick(() => {
    const r = run(file, args, options);
    if (r.error) {
      const err = spawnError('spawn', file, args, r);
      err.cmd = cmd;
      binding.scheduleIo(() => {
        child.emit('error', err);
        if (callback) callback(err, encoding === 'buffer' ? Buffer.alloc(0) : '', encoding === 'buffer' ? Buffer.alloc(0) : '');
        child.emit('close', -2, null);
      }, 0);
      return;
    }
    child.pid = process.pid + 1;
    binding.scheduleIo(() => {
      const stdout = decode(r.stdout, encoding);
      const stderr = decode(r.stderr, encoding);
      if (r.stdout.length) child.stdout.push(stdout);
      child.stdout.push(null);
      if (r.stderr.length) child.stderr.push(stderr);
      child.stderr.push(null);
      child.exitCode = r.status;
      child.emit('exit', r.status, null);
      let err = null;
      if (r.status !== 0) {
        err = new Error(`Command failed: ${cmd}\n${r.stderr.toString()}`);
        err.code = r.status;
        err.killed = false;
        err.signal = null;
        err.cmd = cmd;
        err.stdout = stdout;
        err.stderr = stderr;
      }
      if (callback) callback(err, stdout, stderr);
      child.emit('close', r.status, null);
    }, r.elapsedMs);
  });
  return child;
}

function exec(command, options, callback) {
  if (typeof options === 'function') { callback = options; options = {}; }
  return execFile(String(command), [], { ...(options || {}), shell: true }, callback);
}

function fork(modulePath, args, options) {
  [modulePath, args, options] = normalizeArgs(modulePath, args, options);
  return spawn(process.execPath || '/usr/bin/node', [modulePath, ...args], { stdio: 'inherit', ...options });
}

const promisify = require('util').promisify;
const execP = (cmd, opts) => new Promise((resolve, reject) => {
  exec(cmd, opts || {}, (err, stdout, stderr) => (err ? reject(Object.assign(err, { stdout, stderr })) : resolve({ stdout, stderr })));
});
const execFileP = (file, args, opts) => new Promise((resolve, reject) => {
  execFile(file, args || [], opts || {}, (err, stdout, stderr) => (err ? reject(Object.assign(err, { stdout, stderr })) : resolve({ stdout, stderr })));
});
exec[promisify.custom] = execP;
execFile[promisify.custom] = execFileP;

module.exports = { ChildProcess, exec, execSync, execFile, execFileSync, spawn, spawnSync, fork };
