set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# Start the minikube
minikube-start:
    minikube status >/dev/null 2>&1 || minikube start
    kubectl wait --for=condition=Ready nodes --all --timeout=120s

# Build a Docker image inside the minikube env
minikube-build image="that-limit:dev":
    minikube image build -t {{ image }} .

# Apply changes to create resourses in the cluster
minikube-deploy: minikube-build
    kubectl apply -f k8s/

# Start-build-deploy in one go
minikube-up: minikube-start minikube-deploy
    kubectl wait \
      --for=condition=Ready pod \
      -l app=that-limit \
      --timeout=120s
    minikube service that-limit-lb

# Start dependencies in Docker (redis, etc)
dev-up compose_file="docker-compose.dev.yml":
    docker compose -f {{ compose_file }} up -d

# Stop Docker dependencies
dev-down compose_file="docker-compose.dev.yml":
    docker compose -f {{ compose_file }} down

# Start dependencies in Docker + run Rust app with hot reload
dev: dev-up
    RUST_LOG=trace cargo watch -x run -w src

# Run pedantic linter
clippy:
    cargo fmt
    cargo clippy -- \
        -W clippy::pedantic \
        -W clippy::nursery \
        -W clippy::unwrap_used

# Run tests with coverage
cov:
    cargo llvm-cov \
        --ignore-filename-regex ".*/src/bootstrap.rs|.*/src/main.rs" \
        --open
