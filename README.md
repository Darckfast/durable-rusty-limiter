# durable-rusty-limiter

A simple Rust rate-limiter made for Cloudflare Durable Objects

## Getting Started

Fork it and deploy it to Cloudflare

```bash
pnpx wrangler deploy
```

Change the necessary configuration on `wrangler.toml`
```
[vars]
COOLDOWN_IN_MS = "60000" // cooldown from when the limited is reached, until it gets allowed again
MAX_REQ_PER_SEC = "10" // maxium of request per second before being limited

```

Add the bindings to your worker `wrangler.jsonc`

```json
"durable_objects": {
    "bindings": [
        {
            "name": "RUSTY_LIMITER",
            "script_name": "durable-rusty-limiter",
            "class_name": "RustyLimiter"
        }
    ]
}
```

Then call it in your worker

```ts
let ip = request.headers.get('cf-connecting-ip') || request.headers.get('x-forwarded-for')
let limiter = env.RUSTY_LIMITER.getByName(ip)
let rs = await limiter.fetch("http://rate-limit")

if (rs.ok) {
    // request is ok to proceed
} else {
    // request is being rate-limited
    return new Response(null, { 
        status: rs.status, // limiter.fetch() will return either 200 or 429
        headers: rs.headers, // the returned headers will contain the Retry-After header with the duration in seconds of which the client must wait before they stop being limited
    })
}

```

## Notes
### Same source
In the example above, we use the client's IP as the unique identifier for our durable object, this is important because each durable object is a contained limiter, meaning if a fixed name or ID is used, all calls will share the same limit parameter. But it does not necessarily have to be limited by IP, any identifier that makes sense to your use is well fitted.

### Workers invocations
This durable object needs to be called before your worker processes a request or event. The main goal is to avoid unnecessary request processing at the cost of invoking this durable object on every request.
