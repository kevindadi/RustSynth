# RustSynth - Pushdown CPN Safe Rust Synthesizer
# Docker Build Instructions:
#   docker build -t RustSynth .
#   docker run --rm RustSynth
#   docker run --rm -v $(pwd)/examples:/workspace/examples RustSynth

FROM rust:1.85-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install nightly toolchain for rustdoc JSON
RUN rustup toolchain install nightly --profile minimal

WORKDIR /build

# Copy Cargo files first for caching
COPY Cargo.toml Cargo.lock ./
COPY rust-toolchain.toml ./

# Create dummy src for dependency caching
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies only
RUN cargo build --release 2>/dev/null || true

# Copy actual source
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies and tools
RUN apt-get update && apt-get install -y \
    ca-certificates \
    python3 \
    graphviz \
    && rm -rf /var/lib/apt/lists/*

# Install Rust for rustdoc JSON generation
RUN apt-get update && apt-get install -y curl && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal && \
    rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.cargo/bin:${PATH}"

# Install nightly for rustdoc JSON
RUN rustup toolchain install nightly --profile minimal

WORKDIR /workspace

# Copy binary from builder
COPY --from=builder /build/target/release/RustSynth /usr/local/bin/

# Copy test script and examples
COPY run_tests.py ./
COPY examples ./examples

# Make test script executable
RUN chmod +x run_tests.py

# Default command: run tests
CMD ["python3", "run_tests.py", "--no-build"]
