const net = require('net');

const server = net.createServer(socket => {
  socket.on('data', data => {
    socket.write(data);
  });
});

server.listen(9090, '127.0.0.1', () => {
  console.log('Node.js TCP Echo Server listening on port 9090...');
});
