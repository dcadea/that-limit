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

### Stress test with wrk
```bash
wrk -t4 -c150 -d30s -s ./tests/performance/protected.lua http://127.0.0.1:8000/consume
```
