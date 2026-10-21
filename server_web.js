const http = require('http');

const server = http.createServer((req, res) => {
  res.writeHead(200, {
    'Content-Type': 'text/plain',
    'Content-Length': 13,
    'Connection': 'close',
  });
  res.end('Hello, World!');
});

server.listen(8080, '127.0.0.1', () => {
  console.log('Node.js HTTP Server listening on port 8080...');
});
