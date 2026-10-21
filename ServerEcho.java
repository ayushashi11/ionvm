import java.io.*;
import java.net.*;

public class ServerEcho {
    public static void main(String[] args) throws Exception {
        try (ServerSocket server = new ServerSocket(9090)) {
            System.out.println("Java Virtual Thread Echo Server listening on 9090...");
            while (true) {
                Socket client = server.accept();
                Thread.ofVirtual().start(() -> {
                    try (client;
                         InputStream in = client.getInputStream();
                         OutputStream out = client.getOutputStream()) {
                        byte[] buf = new byte[1024];
                        int n;
                        while ((n = in.read(buf)) != -1) {
                            out.write(buf, 0, n);
                            out.flush();
                        }
                    } catch (IOException ignored) {}
                });
            }
        }
    }
}
