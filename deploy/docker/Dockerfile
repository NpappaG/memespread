FROM rust:slim as builder

# Install build dependencies
RUN apt-get update && \
    apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    g++ \
    curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app
COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    curl \
    libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/target/release/memespread /usr/local/bin/

# Expose both frontend and backend ports
EXPOSE 8000 3000

# Health check for both services
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health && curl -f http://localhost:3000 || exit 1

CMD ["memespread"]
