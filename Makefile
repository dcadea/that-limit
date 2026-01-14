.PHONY: minikube-up minikube-start minikube-deploy minikube-build clippy

minikube-start:
	@minikube status >/dev/null 2>&1 || minikube start

minikube-build:
	minikube image build -t that-limit:dev .

minikube-deploy: minikube-build
	kubectl apply -f k8s/

minikube-up: minikube-start minikube-deploy

clippy:
	cargo fmt && cargo clippy -- \
	-W clippy::pedantic \
	-W clippy::nursery \
	-W clippy::unwrap_used
