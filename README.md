# That Limit
Distributed rate limit service in Rust, backed by Redis. Runs as a plain HTTP service or as an [Envoy rate limit service](https://www.envoyproxy.io/docs/envoy/latest/api-v3/service/ratelimit/v3/rls.proto) (gRPC), so it plugs into Envoy, Emissary, Istio and other Envoy-based gateways.

- ~164k req/s and p99 under 8 ms on a single laptop (see [Benchmarks](#benchmarks-http-adapter-v010)).
- Hot path is lock-free and local: Redis is called once per batch of tokens, not once per request.
- Never admits more than the quota; returns unused tokens to Redis on graceful shutdown.

## How it works

### Limits
Each request is mapped to a bucket:

| Bucket      | Key in Redis   | HTTP adapter source                           | Envoy adapter descriptor |
|-------------|----------------|-----------------------------------------------|--------------------------|
| `protected` | `sub:<sub>`    | `sub` claim of the `Authorization: Bearer` JWT | `user_id`                |
| `public`    | `ip:<ip>`      | first `X-Forwarded-For` entry, else `X-Real-IP` | `remote_address`         |

Each bucket type has a policy (see [`static/config.json`](static/config.json)): `quota` requests per `reset_in` seconds. This is a **fixed window**: the window starts with the first request for a key and the full quota comes back when the Redis key expires.

Only requests whose domain is listed in `domains` are limited (the first label of `X-Forwarded-Host` for HTTP, the RLS `domain` for Envoy). Any other domain gets an allow response.

### Leases
Calling Redis on every request would cap throughput at Redis round-trips. Instead, each instance **leases** a batch of `lease_size` tokens and spends them locally:

1. A request arrives for bucket `B`. If the instance holds tokens for `B`, it decrements an `AtomicU64` and answers. No lock, no network.
2. If the local bucket is empty or expired, one task leases more tokens from Redis. Concurrent requests for the same bucket wait on a `tokio::sync::Notify` instead of all calling Redis (single flight).
3. If Redis has no tokens left, the bucket is marked exhausted and requests get `429` (HTTP) or `OVER_LIMIT` (Envoy) with the time until the window resets.

The lease is one Lua script ([`crates/cache/src/action.rs`](crates/cache/src/action.rs)), so read, decrement and TTL happen atomically in Redis even with many instances:

```lua
-- first lease of the window: create the key with quota - lease_size and the window TTL
-- later leases: DECRBY min(remaining, lease_size); return {leased, ttl}
```

The local bucket expires together with the Redis key, so all instances reset in the same window. A background task drops expired buckets every `cleanup.interval` seconds.

### Consistency trade-offs
- **No over-admission.** Tokens leave Redis before they are spent, so the sum of admitted requests across all instances never exceeds `quota` per window.
- **Possible under-admission.** Tokens leased by one instance cannot be spent on another. With `N` instances a client can get `429` while up to `(N - 1) * lease_size` tokens still sit unused elsewhere. Smaller `lease_size` means fairer limits and more Redis calls.
- **Fixed window bursts.** A client can spend a full quota at the end of one window and again at the start of the next.

### Failure modes
| Event                          | Behaviour |
|--------------------------------|-----------|
| Redis unreachable              | Buckets with leased tokens keep serving until the lease runs out or expires. A request that needs a new lease fails: HTTP `500`, Envoy gRPC `INTERNAL`. With Envoy, [`failure_mode_deny`](https://www.envoyproxy.io/docs/envoy/latest/configuration/http/http_filters/rate_limit_filter) decides if that is fail open (Envoy default) or fail closed. The HTTP caller decides the same for itself. |
| Graceful shutdown (`SIGTERM`)  | Unused leased tokens are returned to Redis with `INCRBY` (only when `cleanup.enabled`). |
| Crash / `SIGKILL`              | Leased tokens are lost until the window resets: under-admission, never over-admission. |
| Config file missing or invalid | Falls back to built-in defaults with no domains, so nothing is limited. |

### Trust assumptions
The service sits behind a gateway and trusts it:
- JWTs are **decoded, not verified**. Validate them at the gateway before the request reaches That Limit.
- Client IPs come from `X-Forwarded-For` / `X-Real-IP`. The gateway must overwrite these headers, or clients can pick their own bucket.

## Getting started

### Prerequisites
- :crab: [Rust](https://www.rust-lang.org/tools/install)
- :whale: [Docker](https://www.docker.com/get-started) for Redis
- :gear: [just](https://github.com/casey/just) for the recipes below
- ⎈ [Minikube](https://minikube.sigs.k8s.io/docs/start) to run in a local cluster (optional)

### Build and run
The server is built with exactly one adapter feature: `http` or `envoy`. The build fails without one.
```bash
git clone git@github.com:dcadea/that-limit.git
cd that-limit
docker compose -f docker-compose.http.yml up -d redis   # Redis on localhost:6379
cargo run -p that-limit-server --features http          # or --features envoy
```

```bash
cargo test                  # needs Docker: tests start Redis with testcontainers
just clippy                 # fmt + pedantic clippy
just cov                    # tests with coverage
just dev                    # hot reload (cargo install cargo-watch)
```

Try it:
```bash
curl -i -X POST http://127.0.0.1:8000/consume \
  -H 'X-Forwarded-Host: that-limit.local' \
  -H 'X-Forwarded-For: 203.0.113.7'
# 200 with `ratelimit: r=<tokens left>`, or 429 with `Retry-After`
```

### With Docker
```bash
just docker http            # or: just docker envoy
```

### With Minikube
```bash
just minikube http          # or: just minikube envoy (installs Emissary)
```
Run `minikube dashboard` to inspect the cluster.

### Configuration
Environment variables (a `.env` file is loaded if present):
```dotenv
CFG_PATH=static/config.json # required
RUST_LOG=info
HTTP_PORT=8000
ENVOY_PORT=50051
REDIS_HOST=127.0.0.1
REDIS_PORT=6379
```

Policy file (`CFG_PATH`):
```json
{
  "domains": ["that-limit"],
  "cleanup": { "enabled": true, "interval": 60 },
  "protected": { "criteria": "sub", "quota": 10000, "lease_size": 2000, "reset_in": 600 },
  "public":    { "criteria": "ip",  "quota": 500,   "lease_size": 100,  "reset_in": 3600 }
}
```
`interval` and `reset_in` are in seconds.

## Benchmarks (HTTP adapter, v0.1.0)
Setup: one Apple M1 Pro (10 cores, 8P + 2E, 32 GB). Load generator, service and Redis all ran on the **same machine**, so they compete for CPU and the numbers are an upper bound for a single node, not a cluster figure. Most requests hit the local lease, so Redis is touched roughly once per `lease_size` requests.

No baseline yet: a run of [envoyproxy/ratelimit](https://github.com/envoyproxy/ratelimit) on the same box with the same load is planned.

### wrk
```
$ wrk -t4 -c150 -d30s -s ./tests/performance/protected.lua http://127.0.0.1:8000/consume

Running 30s test @ http://127.0.0.1:8000/consume
  4 threads and 150 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   839.02us  143.68us   8.80ms   87.40%
    Req/Sec    41.22k     1.00k   43.17k    91.42%
  4922963 requests in 30.01s, 437.30MB read
Requests/sec: 164065.28
Transfer/sec:     14.57MB
```
### k6
```bash
k6 run tests/performance/protected.js
```

100 Virtual users:
```
█ THRESHOLDS

http_req_duration
✓ 'p(99)<10' p(99)=2.8ms

http_req_failed
✓ 'rate<0.01' rate=0.00%

█ TOTAL RESULTS

checks_total.......: 13273019 110608.300867/s
checks_succeeded...: 100.00%  13273019 out of 13273019
checks_failed......: 0.00%    0 out of 13273019

✓ is status 200 or 429

HTTP
http_req_duration..............: avg=586.33µs min=21µs   med=416µs    max=46.62ms p(90)=1.24ms p(95)=1.64ms
{ expected_response:true }...: avg=586.33µs min=21µs   med=416µs    max=46.62ms p(90)=1.24ms p(95)=1.64ms
http_req_failed................: 0.00%    0 out of 13273019
http_reqs......................: 13273019 110608.300867/s

EXECUTION
iteration_duration.............: avg=654.79µs min=37.5µs med=472.25µs max=46.78ms p(90)=1.35ms p(95)=1.79ms
iterations.....................: 13273019 110608.300867/s
vus............................: 1        min=1             max=100
vus_max........................: 100      min=100           max=100

NETWORK
data_received..................: 1.2 GB   10 MB/s
data_sent......................: 1.5 GB   12 MB/s
```

500 Virtual users:
```
█ THRESHOLDS

  http_req_duration
  ✓ 'p(99)<10' p(99)=7.68ms

  http_req_failed
  ✓ 'rate<0.01' rate=0.00%

█ TOTAL RESULTS

  checks_total.......: 12201053 101675.096819/s
  checks_succeeded...: 100.00%  12201053 out of 12201053
  checks_failed......: 0.00%    0 out of 12201053

  ✓ is status 200 or 429

  HTTP
  http_req_duration..............: avg=2.01ms min=21µs    med=1.69ms max=103.73ms p(90)=4.08ms p(95)=4.9ms
    { expected_response:true }...: avg=2.01ms min=21µs    med=1.69ms max=103.73ms p(90)=4.08ms p(95)=4.9ms
  http_req_failed................: 0.00%    0 out of 12201053
  http_reqs......................: 12201053 101675.096819/s

  EXECUTION
  iteration_duration.............: avg=2.97ms min=38.95µs med=2.71ms max=119.23ms p(90)=5.39ms p(95)=6.76ms
  iterations.....................: 12201053 101675.096819/s
  vus............................: 1        min=1             max=500
  vus_max........................: 500      min=500           max=500

  NETWORK
  data_received..................: 1.1 GB   9.5 MB/s
  data_sent......................: 1.4 GB   11 MB/s
```
