#!/usr/bin/env node
/** The site, served the way a real host serves it, for looking at the download overlay.
 *
 *   node research/gallery/chrome/serve.mjs [port] [--gzip]
 *
 * scripts/serve-site.mjs answers with `Transfer-Encoding: chunked` and no `Content-Length`,
 * so nothing there can be counted; this one sets it. With `--gzip` it compresses the
 * module and the world the way GitHub Pages does — `Content-Encoding: gzip` and a
 * `Content-Length` that counts the bytes on the wire, not the ones a reader gets back —
 * which is the case where the overlay has no honest percentage to show.
 */
import {createServer} from 'node:http';
import {readFile, stat} from 'node:fs/promises';
import {gzipSync} from 'node:zlib';
import {resolve, extname, sep} from 'node:path';

// SITE_ROOT is for standing this round's site next to the one before it and timing both.
const root = process.env.SITE_ROOT
  ? resolve(process.env.SITE_ROOT)
  : resolve(new URL('../../../', import.meta.url).pathname, 'site');
const args = process.argv.slice(2);
const port = Number(args.find(a => /^\d+$/.test(a)) ?? 8123);
const squeeze = args.includes('--gzip');
const types = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.json': 'application/json', '.wasm': 'application/wasm', '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg', '.png': 'image/png', '.svg': 'image/svg+xml',
  '.ttf': 'font/ttf', '.woff2': 'font/woff2', '.ico': 'image/x-icon',
  '.md': 'text/markdown; charset=utf-8', '.txt': 'text/plain; charset=utf-8',
};
const cache = new Map();

createServer(async (request, response) => {
  try {
    const url = new URL(request.url, 'http://localhost');
    // Somebody else's page, with the slideshow framed in it. Served from here rather
    // than set into an about:blank document, because a frame pointed at http from an
    // opaque origin never loads at all.
    if (url.pathname === '/framed.html') {
      const how = url.searchParams.get('how') ?? 'allow="fullscreen"';
      const body = `<!doctype html><html lang="en"><meta charset="utf-8">
<title>Somebody else's page</title>
<style>html,body{margin:0;height:100%;background:#1b1a18;display:grid;place-items:center;
  font:14px/1.5 system-ui,sans-serif;color:#9a948a}
 iframe{width:960px;height:620px;border:0;border-radius:10px}</style>
<p style="position:fixed;top:18px;left:0;right:0;text-align:center">somebody else's page</p>
<iframe src="./embed.html" title="ComputerWorld" ${how}></iframe>`;
      const out = Buffer.from(body, 'utf8');
      response.writeHead(200, {'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store', 'content-length': String(out.length)});
      response.end(request.method === 'HEAD' ? undefined : out);
      return;
    }
    let path = resolve(root, '.' + decodeURIComponent(url.pathname));
    if (path !== root && !path.startsWith(root + sep)) throw new Error('outside the site');
    if ((await stat(path)).isDirectory()) path = resolve(path, 'index.html');
    let bytes = await readFile(path);
    const head = {'content-type': types[extname(path)] ?? 'application/octet-stream', 'cache-control': 'no-store'};
    const big = ['.wasm', '.js'].includes(extname(path)) && bytes.length > 100_000;
    if (squeeze && big && (request.headers['accept-encoding'] ?? '').includes('gzip')) {
      if (!cache.has(path)) cache.set(path, gzipSync(bytes, {level: 6}));
      bytes = cache.get(path);
      head['content-encoding'] = 'gzip';
      head['vary'] = 'Accept-Encoding';
    }
    head['content-length'] = String(bytes.length);
    response.writeHead(200, head);
    response.end(request.method === 'HEAD' ? undefined : bytes);
  } catch {
    response.writeHead(404, {'content-type': 'text/plain', 'content-length': '10'});
    response.end('Not found\n');
  }
}).listen(port, '127.0.0.1', () => console.log(`site → http://127.0.0.1:${port}/  ${squeeze ? '(gzip)' : '(identity)'}`));
