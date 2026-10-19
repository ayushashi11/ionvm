const { Worker, isMainThread, parentPort, workerData } = require('worker_threads');

// 1. Dynamic Object as cache (matching IonVM's new_object + set_prop):
const OBJECT_CACHE = {
    0: 0,
    1: 1,
    2: 4,
    3: 27,
    4: 256,
    5: 3125,
    6: 46656,
    7: 823543,
    8: 16777216,
    9: 387420489
};

// 2. Standard JS Array as cache:
const ARRAY_CACHE = [0, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489];

function isMunchausen(n, cache) {
    if (n === 0) return false;
    let temp = n;
    let sum = 0;
    while (temp > 0) {
        const digit = temp % 10;
        sum += cache[digit];
        if (sum > n) return false;
        temp = Math.floor(temp / 10);
    }
    return sum === n;
}

if (!isMainThread) {
    const { start, end, useArray } = workerData;
    const cache = useArray ? ARRAY_CACHE : OBJECT_CACHE;
    const found = [];
    for (let n = start; n < end; n++) {
        if (isMunchausen(n, cache)) {
            found.push(n);
        }
    }
    parentPort.postMessage(found);
} else {
    async function runBenchmark(numWorkers = 8, useArray = false) {
        const MAX = 10_000_000;
        const modeName = (useArray ? 'Array' : 'Object') + ` (${numWorkers} worker${numWorkers > 1 ? 's' : ''})`;

        const startTime = process.hrtime.bigint();

        if (numWorkers === 1) {
            const cache = useArray ? ARRAY_CACHE : OBJECT_CACHE;
            const found = [];
            for (let n = 0; n < MAX; n++) {
                if (isMunchausen(n, cache)) {
                    found.push(n);
                }
            }
            const endTime = process.hrtime.bigint();
            const elapsed = Number(endTime - startTime) / 1e9;
            return { modeName, found, elapsed };
        }

        const chunk = Math.floor(MAX / numWorkers);
        const promises = [];

        for (let i = 0; i < numWorkers; i++) {
            const start = i * chunk;
            const end = (i === numWorkers - 1) ? MAX : (i + 1) * chunk;
            promises.push(new Promise((resolve, reject) => {
                const worker = new Worker(__filename, {
                    workerData: { start, end, useArray }
                });
                worker.on('message', resolve);
                worker.on('error', reject);
            }));
        }

        const results = await Promise.all(promises);
        const endTime = process.hrtime.bigint();
        const elapsed = Number(endTime - startTime) / 1e9;
        const found = results.flat().sort((a, b) => a - b);
        return { modeName, found, elapsed };
    }

    async function main() {
        console.log("=== Node.js v" + process.versions.node + " Munchausen Benchmark (N = 10,000,000) ===\n");

        // 1. Multi-threaded (8 workers) with Object cache (like IonVM)
        const res8Obj = await runBenchmark(8, false);
        console.log(`[1] 8 Workers + Object { "0": 0, ... } (IonVM style):`);
        console.log(`    Results: ${res8Obj.found.join(', ')}`);
        console.log(`    Time:    ${res8Obj.elapsed.toFixed(4)}s\n`);

        // 2. Multi-threaded (8 workers) with Array cache [0, 1, 4, ...]
        const res8Arr = await runBenchmark(8, true);
        console.log(`[2] 8 Workers + Array [0, 1, 4, ...]:`);
        console.log(`    Results: ${res8Arr.found.join(', ')}`);
        console.log(`    Time:    ${res8Arr.elapsed.toFixed(4)}s\n`);

        // 3. Single-threaded with Object cache
        const res1Obj = await runBenchmark(1, false);
        console.log(`[3] 1 Thread  + Object { "0": 0, ... } (IonVM style):`);
        console.log(`    Results: ${res1Obj.found.join(', ')}`);
        console.log(`    Time:    ${res1Obj.elapsed.toFixed(4)}s\n`);

        // 4. Single-threaded with Array cache
        const res1Arr = await runBenchmark(1, true);
        console.log(`[4] 1 Thread  + Array [0, 1, 4, ...]:`);
        console.log(`    Results: ${res1Arr.found.join(', ')}`);
        console.log(`    Time:    ${res1Arr.elapsed.toFixed(4)}s\n`);
    }

    main();
}
