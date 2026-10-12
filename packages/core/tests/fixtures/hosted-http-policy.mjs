import assert from 'node:assert/strict';
import net from 'node:net';
import http from 'node:http';
// Real guest TCP and HTTP APIs. No parser doubles or external network service.
async function bodyFor(lineBytes) {
    const sockets = new Set();
    const server = net.createServer(socket => {
        sockets.add(socket);
        socket.once('close', () => sockets.delete(socket));
        socket.once('error', error => { if (error.code !== 'ECONNRESET') { console.error(error); process.exitCode = 1; } });
        socket.end('HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n' +
            '0'.repeat(lineBytes - 1) + '1\r\nA\r\n0\r\n\r\n');
    });
    try {
        await new Promise((resolve, reject) => {
            server.once('error', reject);
            server.listen(0, '127.0.0.1', resolve);
        });
        return await new Promise((resolve, reject) => {
            const request = http.get({host: '127.0.0.1', port: server.address().port, path: '/', agent: false}, response => {
                let text = '';
                response.on('error', reject);
                response.once('aborted', () => reject(new Error('hosted HTTP response aborted')));
                response.on('data', chunk => { text += chunk.toString(); if (text.length > 1) { request.destroy(); reject(new Error('hosted HTTP body exceeded1byte')); } });
                response.on('end', () => resolve(text));
            });
            request.setTimeout(5000, () => request.destroy(new Error('hosted HTTP control exceeded5s')));
            request.once('error', reject);
        });
    } finally {
        for (const socket of sockets) socket.destroy();
        await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    }
}
if (__RAISED__) {
    assert.equal(await bodyFor(16417), 'A');
    await assert.rejects(bodyFor(32769), /HTTP chunk size line|limits\.http\.maxChunkSizeLineBytes/);
} else await assert.rejects(bodyFor(16417), /HTTP chunk size line|limits\.http\.maxChunkSizeLineBytes/);
console.log('hosted-http-policy-complete');
