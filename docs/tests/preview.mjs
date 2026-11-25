import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, resolve, sep } from 'node:path';

const distDir = resolve(process.cwd(), 'dist');
const port = Number(process.env.DOCS_TEST_PORT ?? 8000);
const notFoundBody = await readFile(resolve(distDir, '404.html'));

const mimeTypes = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.ico': 'image/x-icon',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.webp': 'image/webp',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.xml': 'application/xml; charset=utf-8',
};

async function resolveFile(pathname) {
  let decoded;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const relative = decoded.replace(/^\/+/, '');
  const candidates = relative === ''
    ? ['index.html']
    : relative.endsWith('/')
      ? [`${relative}index.html`]
      : [relative, `${relative}.html`, `${relative}/index.html`];

  for (const candidate of candidates) {
    const filePath = resolve(distDir, candidate);
    if (filePath !== distDir && !filePath.startsWith(`${distDir}${sep}`)) continue;
    try {
      const info = await stat(filePath);
      if (info.isFile()) return filePath;
    } catch {
      // Try next candidate.
    }
  }
  return null;
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', `http://localhost:${port}`);
  const filePath = await resolveFile(url.pathname);
  if (!filePath) {
    const isAsset = url.pathname.includes('/assets/');
    res.writeHead(404, isAsset ? {} : { 'content-type': 'text/html; charset=utf-8' });
    if (req.method !== 'HEAD' && !isAsset) res.write(notFoundBody);
    res.end();
    return;
  }

  const contentType = mimeTypes[extname(filePath).toLowerCase()] ?? 'application/octet-stream';
  const cacheControl = url.pathname.includes('/assets/')
    ? 'public, max-age=31536000, immutable'
    : 'no-cache';
  const body = await readFile(filePath);
  res.writeHead(200, {
    'cache-control': cacheControl,
    'content-length': body.byteLength,
    'content-type': contentType,
  });
  if (req.method !== 'HEAD') res.write(body);
  res.end();
});

server.listen(port, 'localhost');
