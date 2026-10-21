import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;

public class ServerShout {
    public static void main(String[] args) throws Exception {
        try (ServerSocket server = new ServerSocket(8088)) {
            System.out.println("Java Virtual Thread Shout Server listening on 8088...");
            while (true) {
                Socket client = server.accept();
                Thread.ofVirtual().start(() -> {
                    try (client;
                         InputStream in = client.getInputStream();
                         OutputStream out = client.getOutputStream()) {
                        byte[] buf = new byte[4096];
                        int read = in.read(buf);
                        if (read > 0) {
                            String req = new String(buf, 0, read, StandardCharsets.UTF_8);
                            int bodyIdx = req.indexOf("\r\n\r\n");
                            String body = (bodyIdx != -1) ? req.substring(bodyIdx + 4) : "";
                            String shout = "I WILL SHOUT " + body + " BACK!";
                            byte[] payload = shout.getBytes(StandardCharsets.UTF_8);
                            String header = "HTTP/1.1 200 OK\r\n" +
                                            "Content-Type: text/plain\r\n" +
                                            "Content-Length: " + payload.length + "\r\n" +
                                            "Connection: close\r\n\r\n";
                            out.write(header.getBytes(StandardCharsets.UTF_8));
                            out.write(payload);
                            out.flush();
                        }
                    } catch (IOException ignored) {}
                });
            }
        }
    }
}
