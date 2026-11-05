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
  "img-src 'self' blob:",
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

/** The file for a request path, or null (S2, S3). */
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
 * Starts the site. `api` is `{ port, allowedHost? }` (the real server) or a
 * function `(req, res, url) => void` (a fake). Every request is logged.
 *
 * With `allowedHost`, the proxy does what the tunnel does in front of the
 * real server: it forwards `Host: <allowedHost>`, and the page's
 * own `Origin` as `https://<allowedHost>` (the page's origin through the
 * tunnel). Any other `Origin` passes unchanged, so a foreign one is still
 * refused.
 */
export async function startSite(api) {
  const log = [];
  // How many of the next POSTs lose their answer: the server finishes them, then the connection drops.
  const drops = { post: 0 };
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://site.invalid');
    const entry = { method: req.method, path: url.pathname, search: url.search, headers: req.headers, at: Date.now() };
    log.push(entry);
    if (url.pathname.startsWith('/api/')) {
      if (typeof api === 'function') return api(req, res, url);
      const drop = req.method === 'POST' && drops.post > 0;
      if (drop) drops.post--;
      return proxy(req, res, api.port, api.allowedHost ?? null, `http://${req.headers.host}`, drop);
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
    /** The next `n` POSTs reach the server and finish there, but their answers never reach the page (a lost acknowledgement). */
    dropPostAnswers: (n = 1) => {
      drops.post += n;
    },
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

function proxy(req, res, port, allowedHost, siteOrigin, dropAnswer = false) {
  const headers = { ...req.headers, host: allowedHost ?? `127.0.0.1:${port}`, connection: 'close' };
  if (allowedHost && headers.origin === siteOrigin) headers.origin = `https://${allowedHost}`;
  const up = httpRequest(
    {
      host: '127.0.0.1',
      port,
      method: req.method,
      path: req.url,
      headers,
    },
    (r) => {
      if (dropAnswer) {
        // The server has answered: drop the connection instead of passing the answer on.
        r.resume();
        r.on('end', () => res.socket?.destroy());
        return;
      }
      res.writeHead(r.statusCode ?? 502, r.headers);
      r.pipe(res);
    },
  );
  up.on('error', () => {
    if (!res.headersSent) res.writeHead(502, { 'Content-Type': 'text/plain' });
    res.end('proxy error');
  });
  // The client gave up: so does the proxy. (Not the request's own `close`, which a piped body ends early.)
  res.on('close', () => {
    if (!res.writableEnded) up.destroy();
  });
  // A request body (the send's) goes through as it came.
  req.pipe(up);
}
