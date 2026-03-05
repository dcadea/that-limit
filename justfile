set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# Start in minikube cluster
minikube provider="http":
    minikube status >/dev/null 2>&1 || minikube start
    kubectl wait --for=condition=Ready nodes --all --timeout=120s

    if [ "{{ provider }}" = "envoy" ]; then \
        kubectl get ns emissary >/dev/null 2>&1 || just install-emissary; \
    fi

    docker build --build-arg provider={{ provider }} -t that-limit-{{ provider }}:dev .
    minikube image load that-limit-{{ provider }}:dev

    kubectl apply -f k8s/{{ provider }}

    kubectl wait \
        --for=condition=Ready pod \
        -n that-limit-envoy-ns \
        -l app=that-limit-{{ provider }} \
        --timeout=120s

install-emissary:
    kubectl apply -f https://app.getambassador.io/yaml/emissary/3.9.0/emissary-crds.yaml
    kubectl wait --for=condition=Established crd/ratelimitservices.getambassador.io --timeout=180s
    kubectl wait --for=condition=Established crd/mappings.getambassador.io --timeout=180s

    kubectl create namespace emissary --dry-run=client -o yaml | kubectl apply -f -

    kubectl apply -f https://app.getambassador.io/yaml/emissary/3.9.0/emissary-emissaryns.yaml
    kubectl wait -n emissary --for=condition=Available deployment/emissary-ingress --timeout=180s

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
        --ignore-filename-regex ".*/src/app.rs|.*/src/main.rs" \
        --open
