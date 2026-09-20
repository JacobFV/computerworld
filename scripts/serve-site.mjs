/** Preview the project site locally, with the media types the browser needs.
 *
 * Python's http.server sends `.wasm` as application/octet-stream, which drops the
 * bundle off its streaming-instantiation path; this serves the same bytes GitHub
 * Pages does, and never caches, so a reload always shows the file on disk.
 *
 *   node scripts/serve-site.mjs [port]
 */
import {createServer} from 'node:http';
import {readFile, stat} from 'node:fs/promises';
import {resolve, extname, sep} from 'node:path';

const root = resolve(new URL('..', import.meta.url).pathname, 'site');
const port = Number(process.argv[2] ?? 8000);
const types = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.json': 'application/json', '.wasm': 'application/wasm', '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg', '.png': 'image/png', '.svg': 'image/svg+xml',
  '.ttf': 'font/ttf', '.woff2': 'font/woff2', '.ico': 'image/x-icon',
  '.md': 'text/markdown; charset=utf-8', '.txt': 'text/plain; charset=utf-8',
};

createServer(async (request, response) => {
  try {
    const url = new URL(request.url, 'http://localhost');
    let path = resolve(root, '.' + decodeURIComponent(url.pathname));
    if (path !== root && !path.startsWith(root + sep)) throw new Error('outside the site');
    if ((await stat(path)).isDirectory()) path = resolve(path, 'index.html');
    const bytes = await readFile(path);
    response.writeHead(200, {
      'content-type': types[extname(path)] ?? 'application/octet-stream',
      'cache-control': 'no-store',
    });
    response.end(bytes);
  } catch {
    response.writeHead(404, {'content-type': 'text/plain'});
    response.end('Not found. Build the machines first: bash scripts/build-wasm.sh && node scripts/build-live-world.mjs\n');
  }
}).listen(port, '127.0.0.1', () => {
  console.log(`site → http://localhost:${port}/`);
});
