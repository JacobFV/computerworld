'use strict';
// Subprocesses cannot be spawned from the simulated interpreter.

const EventEmitter = require('events');

function spawnError(syscall, file, args) {
  const e = new Error(`${syscall} ${file} ENOSYS`);
  e.errno = -38;
  e.code = 'ENOSYS';
  e.syscall = `${syscall} ${file}`;
  e.path = file;
  e.spawnargs = args;
  return e;
}

function execSync(cmd) {
  throw spawnError('spawnSync', '/bin/sh', ['-c', String(cmd)]);
}

function execFileSync(file, args = []) {
  throw spawnError('spawnSync', file, args);
}

function spawnSync(file, args = []) {
  const error = spawnError('spawnSync', file, args);
  return { error, status: null, signal: null, output: null, pid: 0, stdout: null, stderr: null };
}

function childLike(file, args, cb) {
  const child = new EventEmitter();
  child.pid = undefined;
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.stdin = { write() { return false; }, end() {} };
  child.kill = () => false;
  const err = spawnError('spawn', file, args);
  process.nextTick(() => {
    if (cb) cb(err, '', '');
    child.emit('error', err);
    child.emit('close', -38);
  });
  return child;
}

function exec(cmd, opts, cb) {
  if (typeof opts === 'function') cb = opts;
  return childLike('/bin/sh', ['-c', String(cmd)], cb);
}

function execFile(file, args, opts, cb) {
  if (typeof args === 'function') cb = args;
  else if (typeof opts === 'function') cb = opts;
  return childLike(file, Array.isArray(args) ? args : [], cb);
}

function spawn(file, args) {
  return childLike(file, Array.isArray(args) ? args : [], null);
}

module.exports = { exec, execSync, execFile, execFileSync, spawn, spawnSync, fork: spawn };
