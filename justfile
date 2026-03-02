set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# Start in minikube cluster
minikube provider="http":
    minikube status >/dev/null 2>&1 || minikube start
    kubectl wait --for=condition=Ready nodes --all --timeout=120s

    docker build --build-arg provider={{ provider }} -t that-limit-{{ provider }}:dev .
    minikube image load that-limit-{{ provider }}:dev

    kubectl apply -f k8s/{{ provider }}

    kubectl wait \
      --for=condition=Ready pod \
      -l app=that-limit-{{ provider }} \
      --timeout=120s
    # minikube service that-limit-{{ provider }}-lb

# Start in docker
docker provider="http":
    docker compose -f docker-compose.{{ provider }}.yml up -d

# Run app server with hot reload
dev feature="http":
    RUST_LOG=trace cargo watch -x run -w crates --features {{ feature }}

# Run pedantic linter
clippy:
    cargo fmt
    cargo clippy --all-features -- \
        -W clippy::pedantic \
        -W clippy::nursery \
        -W clippy::unwrap_used

# Run tests with coverage
cov:
    cargo llvm-cov \
        --all-features \
        --ignore-filename-regex ".*/bootstrap.rs|.*/src/main.rs" \
        --open
