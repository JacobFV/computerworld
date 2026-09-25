'use strict';
// Requests from outside the program to a server it listens with: what an
// embedder (a world service running a Node app) uses to answer the world's
// HTTP. The request is handed to the server exactly as a loopback client's is
// (`Server#_handle`), and the reply is kept on the returned exchange.

const wire = require('internal/httpwire');

function listening(port) {
  const s = wire.servers.get(port);
  return !!(s && s._http && s.listening);
}

function dispatch(port, method, path, rawHeaders, body) {
  const server = wire.servers.get(port);
  const exchange = { done: false, status: 0, headers: [], body: null, error: null };
  if (!server || !server._http || !server.listening) {
    exchange.done = true;
    exchange.error = `nothing is listening on port ${port}`;
    return exchange;
  }
  const client = {
    method,
    path,
    _respond(status, message, raw, out) {
      exchange.status = status;
      exchange.headers = raw.map(String);
      exchange.body = out;
      exchange.done = true;
    },
  };
  try {
    server._handle(client, rawHeaders, Buffer.from(body));
  } catch (e) {
    exchange.done = true;
    exchange.error = e && e.stack ? String(e.stack) : String(e);
  }
  return exchange;
}

module.exports = { listening, dispatch };
