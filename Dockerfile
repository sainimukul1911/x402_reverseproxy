# x402 Reverse Proxy - Multi-stage Dockerfile
# Produces a minimal production image (~20MB)

# ============================================
# Stage 1: Build
# ============================================
FROM rust:1.83-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock* ./

# Create dummy src to cache dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn lib() {}" > src/lib.rs

# Build dependencies (cached layer)
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src

# Build the real application
RUN touch src/main.rs src/lib.rs && \
    cargo build --release --bin x402-proxy

# ============================================
# Stage 2: Runtime
# ============================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -r -s /bin/false x402

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/x402-proxy /usr/local/bin/x402-proxy

# Copy default config
COPY config.toml /app/config.toml

# Set ownership
RUN chown -R x402:x402 /app

# Switch to non-root user
USER x402

# Expose default port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command
ENTRYPOINT ["x402-proxy"]
CMD ["--config", "/app/config.toml"]
