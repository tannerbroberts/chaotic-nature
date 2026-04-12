const https = require('https');
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const dir = __dirname;
const cert = path.join(dir, 'cert.pem');
const key = path.join(dir, 'key.pem');

if (!fs.existsSync(cert) || !fs.existsSync(key)) {
  console.log('Generating self-signed certificate...');
  execSync(`openssl req -x509 -newkey rsa:2048 -keyout "${key}" -out "${cert}" -days 365 -nodes -subj "/CN=localhost"`);
}

const MIME = {
  '.html': 'text/html', '.js': 'application/javascript',
  '.wasm': 'application/wasm', '.pck': 'application/octet-stream',
  '.png': 'image/png', '.svg': 'image/svg+xml',
};

const port = process.argv[2] || 8080;

https.createServer({ cert: fs.readFileSync(cert), key: fs.readFileSync(key) }, (req, res) => {
  let url = req.url === '/' ? '/index.html' : req.url;
  const filePath = path.join(dir, url);
  if (!filePath.startsWith(dir)) { res.writeHead(403); res.end(); return; }

  fs.readFile(filePath, (err, data) => {
    if (err) { res.writeHead(404); res.end('Not found'); return; }
    const ext = path.extname(filePath);
    res.writeHead(200, {
      'Content-Type': MIME[ext] || 'application/octet-stream',
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
      'Cache-Control': 'no-cache',
    });
    res.end(data);
  });
}).listen(port, '0.0.0.0', () => {
  console.log(`Serving on https://0.0.0.0:${port}`);
});
