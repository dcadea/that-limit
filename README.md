# That Limit
Rust-based distributed rate limit service.

## Getting started
These instructions will get you a copy of the project up and running on your local machine for development and testing purposes.

### Prerequisites
- :crab: [Rust](https://www.rust-lang.org/tools/install) installed on your machine.
- :whale: [Docker](https://www.docker.com/get-started) to run dependant services.
- :gear: [Make](https://www.gnu.org/software/make/) to run the project with `make` commands.
- ⎈ [Minikube](https://minikube.sigs.k8s.io/docs/start) to run in a test cluster

### Build
```bash
# Clone the repository
git clone git@github.com:angelwing-io/that-limit.git
cd that-limit       # Navigate to the project directory
cargo build         # Build the project
```

### Running and testing
```bash
cargo run           # Run the project
cargo watch -x run  # Run the project with hot-reload
                    # (req: `cargo install cargo-watch`)
cargo test          # Run the tests

make clippy         # Run pedantic linter
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

### With Make
```bash
make minikube-start     # Start the minikube
make minikube-build     # Build a Docker image inside the minikube env
make minikube-deploy    # Apply changes to create resourses in the cluster
make minikube-up        # Start-build-deploy in one go
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
SERVER_PORT=8000

REDIS_HOST=127.0.0.1
REDIS_PORT=6379
```
