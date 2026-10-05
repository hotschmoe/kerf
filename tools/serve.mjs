#!/usr/bin/env node
// Static server: node tools/serve.mjs <dir> [port=8080] [--coop] [--host 0.0.0.0]
// --coop adds COOP/COEP (cross-origin isolation, needed for SharedArrayBuffer/wasm threads).
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const flag = (n) => args.includes(n);
const hostIdx = args.indexOf('--host');
const host = hostIdx >= 0 ? args[hostIdx + 1] : '127.0.0.1';
const pos = args.filter((a, i) => !a.startsWith('--') && !(hostIdx >= 0 && i === hostIdx + 1));
const root = path.resolve(pos[0] || '.');
const port = Number(pos[1] || 8080);
const coop = flag('--coop');

const MIME = {
  '.html': 'text/html; charset=utf-8', '.htm': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png',
  '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.ico': 'image/x-icon',
  '.pdf': 'application/pdf', '.dxf': 'application/dxf', '.txt': 'text/plain; charset=utf-8',
  '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.map': 'application/json',
  '.wgsl': 'text/plain; charset=utf-8', '.bin': 'application/octet-stream',
};

const server = http.createServer((req, res) => {
  let p;
  try { p = decodeURIComponent(new URL(req.url, 'http://x').pathname); } catch { res.writeHead(400).end('bad url'); return; }
  if (p === '/favicon.ico') { res.writeHead(204).end(); return; }
  let file = path.join(root, p);
  if (!file.startsWith(root)) { res.writeHead(403).end('forbidden'); return; }
  try {
    if (fs.statSync(file).isDirectory()) file = path.join(file, 'index.html');
    const st = fs.statSync(file);
    const h = {
      'Content-Type': MIME[path.extname(file).toLowerCase()] || 'application/octet-stream',
      'Content-Length': st.size,
      'Cache-Control': 'no-store',
      'Access-Control-Allow-Origin': '*',
    };
    if (coop) {
      h['Cross-Origin-Opener-Policy'] = 'same-origin';
      h['Cross-Origin-Embedder-Policy'] = 'require-corp';
      h['Cross-Origin-Resource-Policy'] = 'cross-origin';
    }
    res.writeHead(200, h);
    if (req.method === 'HEAD') res.end(); else fs.createReadStream(file).pipe(res);
    console.log(`200 ${req.method} ${p}`);
  } catch {
    res.writeHead(404, { 'Content-Type': 'text/plain' }).end('not found: ' + p);
    console.log(`404 ${req.method} ${p}`);
  }
});
server.listen(port, host, () => console.log(`serving ${root} at http://${host === '0.0.0.0' ? 'localhost' : host}:${port}/${coop ? ' (COOP/COEP on)' : ''}`));
