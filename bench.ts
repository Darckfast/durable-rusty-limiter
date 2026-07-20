import { Miniflare } from "miniflare";

let worker = new Miniflare({
    scriptPath: "./build/index.js",
    modules: true,
    bindings: {
        MAX_REQS: "100",
        COOLDOWN_IN_MS: "300",
    },
    durableObjects: {
        RUSTY_LIMITER: { className: "RustyLimiter", useSQLite: true }
    },
    modulesRules: [
        { type: "CompiledWasm", include: ["**/*.wasm"], fallthrough: true }
    ]
});

await worker.ready

let RUSTY_LIMITER = await worker.getDurableObjectNamespace("RUSTY_LIMITER");
let stub = RUSTY_LIMITER.getByName("ip:127.0.0.6");

async function benchmark(name: string, fn: any, iterations = 100_000) {
    console.log(`Warming up`)
    for (let i = 0; i < Math.min(1_000, iterations); i++) await fn();

    console.log(`Running benchmark`)
    const start = performance.now();
    for (let i = 0; i < iterations; i++) {
        await fn();
    }
    const end = performance.now();

    const totalMs = end - start;
    const opsPerSec = (iterations / totalMs) * 1000;

    console.log(`${name}:`);
    console.log(`  Total: ${totalMs.toFixed(2)}ms`);
    console.log(`  Avg per op: ${(totalMs / iterations).toFixed(6)}ms`);
    console.log(`  Ops/sec: ${Math.round(opsPerSec).toLocaleString()}`);
    console.log();
}

async function call_rusty_limiter() {
    await stub.fetch("http://rusty-limiter");
}

await benchmark('rusty_limiter', call_rusty_limiter);
await worker.dispose()
