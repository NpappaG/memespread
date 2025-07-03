#!/bin/bash

# Function to detect environment
detect_environment() {
    if command -v kubectl >/dev/null 2>&1 && kubectl cluster-info >/dev/null 2>&1; then
        echo "kubernetes"
    else
        echo "docker"
    fi
}

# Function to tear down kubernetes environment
teardown_kubernetes() {
    echo "Tearing down Kubernetes environment..."
    kubectl delete deployment memespread clickhouse
    kubectl delete service memespread clickhouse
    kubectl delete configmap env-config
    kubectl delete pvc --all
    echo "Kubernetes resources removed."
}

# Function to tear down docker environment
teardown_docker() {
    echo "Tearing down Docker environment..."
    docker-compose down -v
    echo "Docker resources removed."
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [command]"
    echo "Commands:"
    echo "  up (default)    - Set up the environment"
    echo "  down            - Tear down the environment"
    echo "  restart         - Restart services"
    echo "  logs           - Show logs"
}

# Setup environment
ENV_TYPE=$(detect_environment)
COMMAND=${1:-"up"}

case $COMMAND in
    "up")
        echo "Detected environment: $ENV_TYPE"

        if [ "$ENV_TYPE" = "kubernetes" ]; then
            echo "Setting up Kubernetes environment..."
            
            # Create ConfigMap from environment variables
            env | grep -E '^(HELIUS_API_KEY)=' > .env.tmp
            kubectl create configmap env-config --from-env-file=.env.tmp
            rm .env.tmp

            # Apply Kubernetes deployments
            kubectl apply -f clickhouse-deployment.yml
            kubectl apply -f app-deployment.yml

            echo "Waiting for pods to be ready..."
            kubectl wait --for=condition=ready pod -l app=memespread --timeout=120s
            
            echo "Services available at:"
            echo "Frontend: http://localhost:3000"
            echo "Backend: http://localhost:8000"
            echo "ClickHouse: http://localhost:8123"

        else
            echo "Setting up Docker environment..."
            docker-compose up -d

            echo "Services available at:"
            echo "Frontend: http://localhost:3000"
            echo "Backend: http://localhost:8000"
            echo "ClickHouse: http://localhost:8123"
        fi
        ;;
    
    "down")
        echo "Detected environment: $ENV_TYPE"
        if [ "$ENV_TYPE" = "kubernetes" ]; then
            teardown_kubernetes
        else
            teardown_docker
        fi
        ;;

    "restart")
        echo "Restarting services in $ENV_TYPE environment..."
        if [ "$ENV_TYPE" = "kubernetes" ]; then
            kubectl rollout restart deployment memespread clickhouse
            kubectl wait --for=condition=ready pod -l app=memespread --timeout=120s
        else
            docker-compose restart
        fi
        echo "Services restarted."
        ;;

    "logs")
        if [ "$ENV_TYPE" = "kubernetes" ]; then
            echo "Showing Kubernetes logs..."
            kubectl logs -f deployment/memespread
        else
            echo "Showing Docker logs..."
            docker-compose logs -f
        fi
        ;;

    *)
        show_usage
        exit 1
        ;;
esac 