#!/bin/bash

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
    
    # Check for docker-specific .env and override/append variables
    if [ -f "${SCRIPT_DIR}/.env" ]; then
        echo "Found docker/.env, applying overrides"
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
        echo "  - deploy/docker/.env for docker-specific overrides"
        rm "$TEMP_ENV_FILE"
        exit 1
    fi
    
    ENV_FILE="$TEMP_ENV_FILE"
    echo "Using merged environment configuration"
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [command]"
    echo "Commands:"
    echo "  up [--detach|-d]  - Start the services (--detach or -d runs in background)"
    echo "  build            - Build or rebuild the Docker images"
    echo "  down             - Stop and remove all services"
    echo "  restart          - Restart services"
    echo "  logs             - Show logs"
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
COMMAND=${1:-"up"}
DETACH_FLAG=""

case $COMMAND in
    "up")
        echo "Setting up Docker environment..."
        
        # Check if detach flag is provided
        if [ "$2" = "--detach" ] || [ "$2" = "-d" ]; then
            DETACH_FLAG="-d"
        fi
        
        # Check and merge .env files
        check_env_file
        
        # Use the merged env file with docker-compose
        if [ -n "$DETACH_FLAG" ]; then
            docker-compose -f "${SCRIPT_DIR}/docker-compose.yml" --env-file "$ENV_FILE" up $DETACH_FLAG
            echo "Services available at:"
            echo "Frontend: http://localhost:3000"
            echo "Backend: http://localhost:8000"
            echo "ClickHouse: http://localhost:8123"
        else
            echo "Starting services in attached mode (showing logs). Use Ctrl+C to stop."
            echo "To run in background, use: $0 up --detach"
            docker-compose -f "${SCRIPT_DIR}/docker-compose.yml" --env-file "$ENV_FILE" up
        fi
        ;;
    
    "build")
        echo "Building Docker images..."
        docker-compose -f "${SCRIPT_DIR}/docker-compose.yml" build
        ;;

    "down")
        echo "Tearing down Docker environment..."
        docker-compose -f "${SCRIPT_DIR}/docker-compose.yml" down -v
        echo "Docker resources removed."
        ;;

    "restart")
        # Check and merge .env files
        check_env_file
        docker-compose -f "${SCRIPT_DIR}/docker-compose.yml" --env-file "$ENV_FILE" restart
        echo "Services restarted."
        ;;

    "logs")
        docker-compose -f "${SCRIPT_DIR}/docker-compose.yml" logs -f
        ;;

    *)
        show_usage
        exit 1
        ;;
esac 