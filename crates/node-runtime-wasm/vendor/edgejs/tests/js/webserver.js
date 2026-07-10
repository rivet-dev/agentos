const http = require('node:http');

const host = '127.0.0.1';
const port = Number(process.env.PORT || 0);

const server = http.createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8' });
  res.end('hello');
});

server.listen(port, host, () => {
  const address = server.address();
  const resolvedPort = address && typeof address === 'object' ? address.port : port;
  console.log(`webserver listening on port ${resolvedPort}`);
});
