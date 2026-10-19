// Java Sieve of Eratosthenes benchmark
import java.util.Arrays;

public class Sieve {
    public static int countPrimes(int limit) {
        boolean[] isPrime = new boolean[limit + 1];
        Arrays.fill(isPrime, true);
        isPrime[0] = false;
        isPrime[1] = false;

        int sqrtLimit = (int) Math.sqrt(limit);
        for (int p = 2; p <= sqrtLimit; p++) {
            if (isPrime[p]) {
                for (int mult = p * p; mult <= limit; mult += p) {
                    isPrime[mult] = false;
                }
            }
        }

        int count = 0;
        for (int i = 2; i <= limit; i++) {
            if (isPrime[i]) count++;
        }
        return count;
    }

    public static void main(String[] args) {
        int[] limits = {10000000, 50000000};
        for (int limit : limits) {
            long t0 = System.nanoTime();
            int count = countPrimes(limit);
            long t1 = System.nanoTime();
            double ms = (t1 - t0) / 1_000_000.0;
            System.out.printf("Java OpenJDK Sieve (N = %d): %.2f ms [Primes: %d]%n", limit, ms, count);
        }
    }
}
