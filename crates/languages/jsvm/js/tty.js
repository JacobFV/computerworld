'use strict';
// tty: a stream is a terminal when the process's own stdio says so (a program
// run at an in-world terminal); a server or a pipe has none.
const net = require('net');

function streamFor(fd) {
  if (fd === 0) return process.stdin;
  if (fd === 1) return process.stdout;
  if (fd === 2) return process.stderr;
  return null;
}

function isatty(fd) {
  const s = streamFor(Number(fd));
  return !!(s && s.isTTY);
}

class ReadStream extends net.Socket {
  constructor(fd) { super(); this.fd = fd; this.isTTY = true; this.isRaw = false; }
  setRawMode(mode) { this.isRaw = !!mode; return this; }
}

class WriteStream extends net.Socket {
  constructor(fd) { super(); this.fd = fd; this.isTTY = true; this.columns = 80; this.rows = 24; }
  getColorDepth() { return 1; }
  hasColors() { return false; }
  getWindowSize() { return [this.columns, this.rows]; }
  clearLine(dir, cb) { if (cb) process.nextTick(cb); return true; }
  clearScreenDown(cb) { if (cb) process.nextTick(cb); return true; }
  cursorTo(x, y, cb) { if (typeof y === 'function') cb = y; if (cb) process.nextTick(cb); return true; }
  moveCursor(dx, dy, cb) { if (cb) process.nextTick(cb); return true; }
}

module.exports = { isatty, ReadStream, WriteStream };
