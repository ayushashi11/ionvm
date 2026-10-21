import java.io.*;
import java.net.*;

public class ServerWeb {
    public static void main(String[] args) throws Exception {
        byte[] response = ("HTTP/1.1 200 OK\r\n" +
                           "Content-Type: text/plain\r\n" +
                           "Content-Length: 13\r\n" +
                           "Connection: close\r\n\r\n" +
                           "Hello, World!").getBytes();

        try (ServerSocket server = new ServerSocket(8080)) {
            System.out.println("Java Virtual Thread Server listening on 8080...");
            while (true) {
                Socket client = server.accept();
                Thread.ofVirtual().start(() -> {
                    try (client; 
                         InputStream in = client.getInputStream(); 
                         OutputStream out = client.getOutputStream()) {
                        byte[] buf = new byte[1024];
                        in.read(buf);
                        out.write(response);
                        out.flush();
                    } catch (IOException ignored) {}
                });
            }
        }
    }
}
