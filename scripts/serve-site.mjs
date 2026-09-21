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
  '.mp4': 'video/mp4', '.webm': 'video/webm', '.webp': 'image/webp', '.gif': 'image/gif',
};

createServer(async (request, response) => {
  try {
    const url = new URL(request.url, 'http://localhost');
    let path = resolve(root, '.' + decodeURIComponent(url.pathname));
    if (path !== root && !path.startsWith(root + sep)) throw new Error('outside the site');
    if ((await stat(path)).isDirectory()) path = resolve(path, 'index.html');
    const bytes = await readFile(path);
    const type = types[extname(path)] ?? 'application/octet-stream';
    // Ranges, so a <video> can be scrubbed. Without them the browser gets the whole file
    // as one 200 and the scrubber does nothing, which is not what GitHub Pages does and
    // so is a bug you only see locally.
    const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.range ?? '');
    if (range && bytes.length) {
      const last = bytes.length - 1;
      let [, from, to] = range;
      // `bytes=-500` is the final 500 bytes; `bytes=500-` is everything from 500 on.
      let start = from === '' ? Math.max(0, bytes.length - Number(to)) : Number(from);
      let end = from === '' ? last : (to === '' ? last : Math.min(Number(to), last));
      if (Number.isFinite(start) && Number.isFinite(end) && start <= end) {
        response.writeHead(206, {
          'content-type': type,
          'content-range': `bytes ${start}-${end}/${bytes.length}`,
          'content-length': end - start + 1,
          'accept-ranges': 'bytes',
          'cache-control': 'no-store',
        });
        response.end(bytes.subarray(start, end + 1));
        return;
      }
      response.writeHead(416, {'content-range': `bytes */${bytes.length}`});
      response.end();
      return;
    }
    response.writeHead(200, {
      'content-type': type,
      'content-length': bytes.length,
      'accept-ranges': 'bytes',
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
