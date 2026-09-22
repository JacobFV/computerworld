'use strict';
// https: the world's TLS model is the browser's — an https:// request goes to
// port 443 of the host through the world's network, and the world decides
// whether a service answers there. There is no handshake to observe.

const http = require('http');

class Agent extends http.Agent {
  constructor(options = {}) {
    super({ defaultPort: 443, protocol: 'https:', ...options });
  }
}
const globalAgent = new Agent({ keepAlive: true, scheduling: 'lifo', timeout: 5000 });
const defaults = { protocol: 'https:', port: 443, encrypted: true, agent: globalAgent };

function request(input, options, cb) { return new http.ClientRequest(input, options, cb, defaults); }
function get(input, options, cb) {
  const req = request(input, options, cb);
  req.end();
  return req;
}
function createServer(options, listener) { return http.createServer(options, listener); }

module.exports = { Agent, globalAgent, request, get, createServer, Server: http.Server };
