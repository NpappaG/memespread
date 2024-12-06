FROM rust:1.74-slim as builder
WORKDIR /usr/src/app

# Create blank project
RUN cargo new --bin memespread
WORKDIR /usr/src/app/memespread

# Copy manifests
COPY Cargo.lock Cargo.toml ./

# Cache dependencies
RUN cargo build --release
RUN rm src/*.rs

# Copy source code
COPY src ./src

# Build for release
RUN rm ./target/release/deps/memespread*
RUN cargo build --release

# Runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /usr/src/app/memespread/target/release/memespread /usr/local/bin/

# Create non-root user
RUN useradd -m appuser
USER appuser

# Set environment variables
ENV RUST_LOG=info

# Command to run
CMD ["memespread"]
