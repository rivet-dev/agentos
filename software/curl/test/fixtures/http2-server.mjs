// Local real HTTP/2 fixture. Caller supplies ephemeral CA-issued key/certificate.
import http2 from 'node:http2';
import { readFileSync } from 'node:fs';
const [keyPath, certPath] = process.argv.slice(2);
if (!keyPath || !certPath) throw new Error('usage: http2-server.mjs KEY CERT');
const payload = Buffer.alloc(128 * 1024);
for (let i = 0; i < payload.length; i++) payload[i] = i % 251;
const server = http2.createSecureServer({key: readFileSync(keyPath), cert: readFileSync(certPath), allowHTTP1: false, maxSessionMemory: 8});
const sessions = new Set();
server.on('session', session => {
  if (sessions.size >= 8) { session.close(http2.constants.NGHTTP2_REFUSED_STREAM); return; }
  sessions.add(session);
  session.setTimeout(5000, () => session.destroy());
  session.on('error', error => process.stderr.write(`session: ${error.message}\n`));
  session.on('close', () => sessions.delete(session));
});
server.on('stream', (stream, headers) => {
  stream.on('error', error => process.stderr.write(`stream: ${error.message}\n`));
  stream.setTimeout(3000, () => stream.close(http2.constants.NGHTTP2_CANCEL));
  if (headers[':path'] === '/stall') return;
  if (headers[':method'] === 'PUT') {
    let size = 0;
    const chunks = [];
    stream.on('data', chunk => {
      size += chunk.length;
      if (size > payload.length) { stream.close(http2.constants.NGHTTP2_ENHANCE_YOUR_CALM); return; }
      chunks.push(chunk);
    });
    stream.on('end', () => {
      if (stream.destroyed) return;
      const matched = Buffer.concat(chunks).equals(payload);
      stream.respond({':status': matched ? 201 : 422});
      stream.end(matched ? 'matched' : 'mismatch');
    });
    return;
  }
  stream.respond({':status': 200, 'content-length': payload.length});
  stream.end(payload);
});
server.on('error', error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
server.listen(0, '127.0.0.1', () => process.stdout.write(JSON.stringify({port: server.address().port, bytes: payload.length}) + '\n'));
function stop() { for (const session of sessions) session.destroy(); server.close(); }
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
setTimeout(stop, 360000).unref();
