import { Miniflare } from "miniflare";
import assert from "node:assert";
import { after, before, describe, it } from "node:test";

describe('rate-limit', () => {
    let worker: Miniflare
    let RUSTY_LIMITER

    before(async () => {
        worker = new Miniflare({
            scriptPath: "./build/index.js",
            modules: true,
            bindings: {
                MAX_REQ_PER_SEC: "100",
                COOLDOWN_IN_MS: "300",
            },
            durableObjects: {
                RUSTY_LIMITER: "RustyLimiter" // className
            },
            modulesRules: [
                { type: "CompiledWasm", include: ["**/*.wasm"], fallthrough: true }
            ]
        });

        await worker.ready
        RUSTY_LIMITER = await worker.getDurableObjectNamespace("RUSTY_LIMITER");
    })

    after(async () => {
        await worker.dispose()
    })

    describe("stay within limits", () => {
        let status
        before(async () => {
            let stub = RUSTY_LIMITER.getByName("ip:127.0.0.7");
            let rs
            for (let i = 0; i < 99; i++) {
                rs = await stub.fetch("http://rusty-limiter")
                await rs.text()
            }
            status = rs.status
        })

        it('should return 200', () => {
            assert.equal(status, 200)
        })
    })

    describe("exceeded max limit", () => {
        let status
        let headers
        before(async () => {
            let stub = RUSTY_LIMITER.getByName("ip:127.0.0.6");
            let rs
            for (let i = 0; i < 101; i++) {
                rs = await stub.fetch("http://rusty-limiter")
                await rs.text()
            }
            status = rs.status
            headers = rs.headers
        })

        it('should return 429', () => {
            assert.equal(status, 429)
        })

        it('should return Retry-After header', () => {
            assert.equal(headers.get('retry-after'), "0")
        })
    })

    describe("retry after exceeded max limit", () => {
        let status
        before(async () => {
            let stub = RUSTY_LIMITER.getByName("ip:127.0.0.5");
            let rs
            for (let i = 0; i < 102; i++) {
                rs = await stub.fetch("http://rusty-limiter")
                await rs.text()
                if (i === 100) {
                    await new Promise((resolve) => setTimeout(resolve, 300))
                }
            }
            status = rs.status
        })

        it('should return 200', () => {
            assert.equal(status, 200)
        })
    })
})
