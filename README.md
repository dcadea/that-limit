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
cargo run --features http   # Run the project
cargo watch -x run          # Run the project with hot-reload
                            # (req: `cargo install cargo-watch`)
cargo test                  # Run the tests

just clippy                 # Run pedantic linter
just cov                    # Run tests with coverage
```
Optionally you can run the project with `cargo run --release` to enable optimizations.<br>
To run the project in **debug mode**, you can use `RUST_LOG=debug cargo run`.<br>
Or you could just use an IDE like RustRover or Zed :rocket:.


### With Docker
```bash
cd that-limit
docker build -t that-limit:dev .
docker run -d -p 8000:8000 that-limit:dev
```

### With just
Run `just` to see all available recipes:
```text
Available recipes:
    clippy                                         # Run pedantic linter
    cov                                            # Run tests with coverage
    default
    dev feature="http"                             # Start dependencies in Docker + run Rust app with hot reload
    dev-down compose_file="docker-compose.dev.yml" # Stop Docker dependencies
    dev-up compose_file="docker-compose.dev.yml"   # Start dependencies in Docker (redis, etc)
    minikube-build image="that-limit:dev"          # Build a Docker image inside the minikube env
    minikube-deploy                                # Apply changes to create resourses in the cluster
    minikube-start                                 # Start the minikube
    minikube-up                                    # Start-build-deploy in one go
```

Common workflow:

```bash
just dev        # Start local development stack with hot reload
just dev-up     # Start Docker dependencies only
just dev-down   # Stop Docker dependencies
just clippy     # Run formatting + strict linting
```

Additionally you can run `minikube dashboard` to check cluster in web console.

Set up a tunnel to the service by running `minikube service that-limit-lb`. <br>
This will give you an output with the url you can access to reach the service via LB:
| NAMESPACE | NAME          | TARGET PORT | URL                    |
|-----------|---------------|-------------|------------------------|
| default   | that-limit-lb |             | http://127.0.0.1:56706 |

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
