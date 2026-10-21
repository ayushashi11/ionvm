const http = require('http');

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', chunk => {
    body += chunk;
  });
  req.on('end', () => {
    const payload = `I WILL SHOUT ${body} BACK!`;
    res.writeHead(200, {
      'Content-Type': 'text/plain',
      'Content-Length': Buffer.byteLength(payload),
      'Connection': 'close',
    });
    res.end(payload);
  });
});

server.listen(8088, '127.0.0.1', () => {
  console.log('Node.js HTTP Shout Server listening on port 8088...');
});
