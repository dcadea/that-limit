.PHONY: minikube-up minikube-start minikube-deploy minikube-build

minikube-start:
	@minikube status >/dev/null 2>&1 || minikube start

minikube-build:
	minikube image build -t that-limit:dev .

minikube-deploy: minikube-build
	kubectl apply -f k8s/

minikube-up: minikube-start minikube-deploy
