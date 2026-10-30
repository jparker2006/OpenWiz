// A test-only server for `out/`, with the headers the page needs from
// openwiz-server when it serves static files (A8). It mirrors A8; it does not
// prove it. `/api/v1/...` goes to the real server over a fixture scenario, or
// to a fake API.

import { createReadStream, existsSync, lstatSync } from 'node:fs';
import { createServer, request as httpRequest } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';

import { OUT_DIR } from './paths.mjs';

export const PAGE_CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "font-src 'self'",
  "img-src 'self'",
  "connect-src 'self'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join('; ');

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

const COMMON = {
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
  'X-Frame-Options': 'DENY',
  Connection: 'close',
};

/** The file for a request path, or null. */
function fileFor(path) {
  if (path === '/') return join(OUT_DIR, 'index.html');
  if (!/^\/(_next\/static|fonts)\/[A-Za-z0-9._/-]+$/.test(path)) return null;
  if (path.split('/').some((seg, i) => i > 0 && (seg === '' || seg === '.' || seg === '..'))) return null;
  const file = normalize(join(OUT_DIR, path));
  if (!file.startsWith(OUT_DIR + sep)) return null;
  if (!(extname(file) in TYPES)) return null;
  if (!existsSync(file)) return null;
  const st = lstatSync(file);
  if (!st.isFile() || st.isSymbolicLink()) return null;
  return file;
}

/**
 * Starts the site. `api` is `{ port }` (the real server) or a function
 * `(req, res, url) => void` (a fake). Every request is logged.
 */
export async function startSite(api) {
  const log = [];
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://site.invalid');
    const entry = { method: req.method, path: url.pathname, search: url.search, headers: req.headers, at: Date.now() };
    log.push(entry);
    if (url.pathname.startsWith('/api/')) {
      if (typeof api === 'function') return api(req, res, url);
      return proxy(req, res, api.port);
    }
    const file = req.method === 'GET' ? fileFor(url.pathname) : null;
    if (!file) {
      res.writeHead(404, { ...COMMON, 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' });
      return res.end('not found');
    }
    const st = lstatSync(file);
    res.writeHead(200, {
      ...COMMON,
      'Content-Type': TYPES[extname(file)],
      'Content-Length': String(st.size),
      'Content-Security-Policy': PAGE_CSP,
      'Cache-Control': url.pathname.startsWith('/_next/static/') ? 'private, max-age=31536000, immutable' : 'no-store',
    });
    createReadStream(file).pipe(res);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  return {
    origin: `http://127.0.0.1:${port}`,
    log,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

function proxy(req, res, port) {
  const up = httpRequest(
    {
      host: '127.0.0.1',
      port,
      method: req.method,
      path: req.url,
      headers: { ...req.headers, host: `127.0.0.1:${port}`, connection: 'close' },
    },
    (r) => {
      res.writeHead(r.statusCode ?? 502, r.headers);
      r.pipe(res);
    },
  );
  up.on('error', () => {
    if (!res.headersSent) res.writeHead(502, { 'Content-Type': 'text/plain' });
    res.end('proxy error');
  });
  req.on('close', () => {
    if (!res.writableEnded) up.destroy();
  });
  up.end();
}
