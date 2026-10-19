// Node.js Sieve of Eratosthenes benchmark
function sieve(limit) {
    const isPrime = new Uint8Array(limit + 1);
    isPrime.fill(1);
    isPrime[0] = 0;
    isPrime[1] = 0;

    const sqrtLimit = Math.floor(Math.sqrt(limit));
    for (let p = 2; p <= sqrtLimit; p++) {
        if (isPrime[p]) {
            for (let mult = p * p; mult <= limit; mult += p) {
                isPrime[mult] = 0;
            }
        }
    }

    let count = 0;
    for (let i = 2; i <= limit; i++) {
        if (isPrime[i]) count++;
    }
    return count;
}

const limits = [10000000, 50000000];
for (const limit of limits) {
    const t0 = performance.now();
    const count = sieve(limit);
    const t1 = performance.now();
    console.log(`Node.js V8 Sieve (N = ${limit}): ${(t1 - t0).toFixed(2)} ms [Primes: ${count}]`);
}
