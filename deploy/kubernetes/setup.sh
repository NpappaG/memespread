#!/bin/bash

# Function to detect environment
detect_environment() {
    if command -v kubectl >/dev/null 2>&1 && kubectl cluster-info >/dev/null 2>&1; then
        echo "kubernetes"
    else
        echo "docker"
    fi
}

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Function to merge .env files and handle overrides
check_env_file() {
    # Create temporary file for merged environment variables
    TEMP_ENV_FILE=$(mktemp)
    
    # Start with root .env if it exists (base configuration)
    if [ -f "${SCRIPT_DIR}/../../.env" ]; then
        echo "Found root .env file, using as base configuration"
        cat "${SCRIPT_DIR}/../../.env" > "$TEMP_ENV_FILE"
    fi
    
    # Check for kubernetes-specific .env and override/append variables
    if [ -f "${SCRIPT_DIR}/.env" ]; then
        echo "Found kubernetes/.env, applying overrides"
        while IFS= read -r line || [ -n "$line" ]; do
            if [[ $line =~ ^[A-Za-z_][A-Za-z0-9_]*= ]]; then
                # Extract the variable name
                var_name=$(echo "$line" | cut -d'=' -f1)
                # Remove existing setting of this variable if it exists
                sed -i.bak "/^$var_name=/d" "$TEMP_ENV_FILE"
            fi
            echo "$line" >> "$TEMP_ENV_FILE"
        done < "${SCRIPT_DIR}/.env"
    fi
    
    # Check if we have any environment variables
    if [ ! -s "$TEMP_ENV_FILE" ]; then
        echo "Error: No .env files found!"
        echo "Please create a .env file with the required environment variables (e.g., HELIUS_API_KEY=your_key)"
        echo "You can place it in:"
        echo "  - Project root (.env) for base configuration"
        echo "  - deploy/kubernetes/.env for kubernetes-specific overrides"
        rm "$TEMP_ENV_FILE"
        exit 1
    fi
    
    ENV_FILE="$TEMP_ENV_FILE"
    echo "Using merged environment configuration"
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

# Function to show usage
show_usage() {
    echo "Usage: $0 [command]"
    echo "Commands:"
    echo "  up (default)    - Set up the environment"
    echo "  down            - Tear down the environment"
    echo "  restart         - Restart services"
    echo "  logs           - Show logs"
}

# Cleanup function for temporary files
cleanup() {
    if [ -n "$ENV_FILE" ] && [ -f "$ENV_FILE" ]; then
        rm -f "$ENV_FILE"
        rm -f "${ENV_FILE}.bak"
    fi
}

# Register cleanup function
trap cleanup EXIT

# Setup environment
ENV_TYPE=$(detect_environment)
COMMAND=${1:-"up"}

case $COMMAND in
    "up")
        echo "Detected environment: $ENV_TYPE"

        if [ "$ENV_TYPE" = "kubernetes" ]; then
            echo "Setting up Kubernetes environment..."
            
            # Check and merge .env files
            check_env_file
            
            # Create ConfigMap from merged .env file
            kubectl create configmap env-config --from-env-file="$ENV_FILE"

            # Apply Kubernetes deployments
            kubectl apply -f "${SCRIPT_DIR}/clickhouse-deployment.yml"
            kubectl apply -f "${SCRIPT_DIR}/app-deployment.yml"

            echo "Waiting for pods to be ready..."
            kubectl wait --for=condition=ready pod -l app=memespread --timeout=120s
            
            echo "Services available at:"
            echo "Frontend: http://localhost:3000"
            echo "Backend: http://localhost:8000"
            echo "ClickHouse: http://localhost:8123"
        else
            echo "Error: This script is for Kubernetes deployment only."
            echo "For Docker deployment, please use deploy/docker/setup.sh"
            exit 1
        fi
        ;;
    
    "down")
        if [ "$ENV_TYPE" = "kubernetes" ]; then
            teardown_kubernetes
        else
            echo "Error: This script is for Kubernetes deployment only."
            echo "For Docker deployment, please use deploy/docker/setup.sh"
            exit 1
        fi
        ;;

    "restart")
        if [ "$ENV_TYPE" = "kubernetes" ]; then
            kubectl rollout restart deployment memespread clickhouse
            kubectl wait --for=condition=ready pod -l app=memespread --timeout=120s
            echo "Services restarted."
        else
            echo "Error: This script is for Kubernetes deployment only."
            echo "For Docker deployment, please use deploy/docker/setup.sh"
            exit 1
        fi
        ;;

    "logs")
        if [ "$ENV_TYPE" = "kubernetes" ]; then
            echo "Showing Kubernetes logs..."
            kubectl logs -f deployment/memespread
        else
            echo "Error: This script is for Kubernetes deployment only."
            echo "For Docker deployment, please use deploy/docker/setup.sh"
            exit 1
        fi
        ;;

    *)
        show_usage
        exit 1
        ;;
esac 