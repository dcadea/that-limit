# That Limit
Rust-based distributed rate limit service.

## Getting started
These instructions will get you a copy of the project up and running on your local machine for development and testing purposes.

### Prerequisites
- :crab: [Rust](https://www.rust-lang.org/tools/install) installed on your machine.
- :whale: [Docker](https://www.docker.com/get-started) to run dependant services.
- :gear: [just](https://github.com/casey/just) to run the project with `just` commands.
- ⎈ [Minikube](https://minikube.sigs.k8s.io/docs/start) to run in a test cluster

### Build
```bash
# Clone the repository
git clone git@github.com:angelwing-io/that-limit.git
cd that-limit       # Navigate to the project directory
cargo build         # Build the project
```

### Running and testing
Service provides two features: `envoy` and `http`. Each respective feature will serve either http or grpc (envoy compatible) backend. At least one feature should be enabled. If no features are specified, service will default to `http`.
```bash
cargo run \
    -p that-limit-server \
    --features http         # Run the project
cargo watch -x run          # Run the project with hot-reload
                            # (req: `cargo install cargo-watch`)
cargo test                  # Run the tests

just clippy                 # Run pedantic linter
just cov                    # Run tests with coverage
```
Optionally you can run the project with `cargo run -p that-limit-server --release` to enable optimizations.<br>
To run the project in **debug mode**, you can use `RUST_LOG=debug cargo run -p that-limit-server`.<br>
Or you could just use an IDE like RustRover or Zed :rocket:.


### With Docker
```bash
cd that-limit
docker build --build-arg provider=http -t that-limit:dev .
docker run -d -p 8000:8000 that-limit:dev
```

### With just
Run `just` to see all available recipes:
```text
Available recipes:
    clippy                   # Run pedantic linter
    cov                      # Run tests with coverage
    dev feature="http"       # Run app server with hot reload
    docker provider="http"   # Start in docker
    minikube provider="http" # Start in minikube cluster
```

Additionally you can run `minikube dashboard` to check cluster in web console.

### Configuration
Application will not start without the **required** environment configuration. <br>
**Optional** variables have default values, but it is highly recommended to override these once you have a working setup.
- Required environment variables:
```dotenv
N/A
```
- Optional environment variables:
```dotenv
RUST_LOG=info

CFG_PATH=static/config.json
HTTP_PORT=8000
ENVOY_PORT=55051

REDIS_HOST=127.0.0.1
REDIS_PORT=6379
```

## HTTP Adapter stress test
Apple M1 Pro 10 cores (8p + 2e) - 32GB LPDDR5
### wrk
```
$ wrk -t4 -c150 -d30s -s ./tests/performance/protected.lua http://127.0.0.1:8000/consume

Running 30s test @ http://127.0.0.1:8000/consume
  4 threads and 150 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   838.68us  200.34us  12.68ms   89.64%
    Req/Sec    40.85k     2.15k   56.99k    97.17%
  4890280 requests in 30.10s, 438.39MB read
Requests/sec: 162455.13
Transfer/sec:     14.56MB
```
### k6

```
$ k6 run tests/performance/protected.js

         /\      Grafana   /‾‾/
    /\  /  \     |\  __   /  /
   /  \/    \    | |/ /  /   ‾‾\
  /          \   |   (  |  (‾)  |
 / __________ \  |_|\_\  \_____/

     execution: local
        script: tests/performance/protected.js
        output: -

     scenarios: (100.00%) 1 scenario, 500 max VUs, 2m30s max duration (incl. graceful stop):
              * default: Up to 500 looping VUs for 2m0s over 3 stages (gracefulRampDown: 30s, gracefulStop: 30s)


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

running (2m00.0s), 000/500 VUs, 12201053 complete and 0 interrupted iterations
default ✓ [======================================] 000/500 VUs  2m0s
```
