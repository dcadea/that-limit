.PHONY: minikube-up minikube-start minikube-deploy minikube-build dev dev-up dev-down clippy cov

minikube-start:
	@minikube status --format='{{.Host}}' | grep -q Running || minikube start
	@kubectl wait --for=condition=Ready nodes --all --timeout=120s

minikube-build:
	minikube image build -t that-limit:dev . || exit 1

minikube-deploy: minikube-build
	kubectl apply -f k8s/

minikube-up: minikube-start minikube-deploy

dev-up:
	docker-compose -f docker-compose.dev.yml up -d

dev: dev-up
	RUST_LOG=trace cargo watch -x "run" -w src

dev-down:
	docker-compose -f docker-compose.dev.yml down

clippy:
	cargo fmt && cargo clippy -- \
	-W clippy::pedantic \
	-W clippy::nursery \
	-W clippy::unwrap_used

cov:
	cargo llvm-cov --open
