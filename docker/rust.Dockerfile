ARG PACKAGE
ARG CRATE_DIR

# ------------ Planner: compute dependency graph for optimal caching ------------
FROM lukemathwalker/cargo-chef:latest-rust-1.90-slim-bullseye AS chef
WORKDIR /app
ARG PACKAGE
ARG CRATE_DIR

# Only copy manifests first for maximal layer cache hits
COPY Cargo.toml Cargo.lock ./
COPY ${CRATE_DIR}/Cargo.toml ${CRATE_DIR}/Cargo.toml

# Create a minimal binary target so cargo metadata succeeds without sources
RUN mkdir -p ${CRATE_DIR}/src \
 && printf 'fn main() {}\n' > ${CRATE_DIR}/src/main.rs

# Prepare the dependency recipe (no source code yet)
RUN cargo chef prepare --recipe-path recipe.json


# ------------------------------ Builder stage ---------------------------------
FROM lukemathwalker/cargo-chef:latest-rust-1.90-slim-bullseye AS builder
WORKDIR /app
ARG PACKAGE

# Minimal native deps and certs for HTTPS during build
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
    pkg-config \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Reuse the dependency layer cache
COPY --from=chef /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

# Now bring in the full source and build the binary
COPY . .
RUN cargo build -p ${PACKAGE} --release --locked


# ------------------------------ Runtime stage ---------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
WORKDIR /app
ARG PACKAGE

# Copy CA bundle for outbound HTTPS just in case dependencies require it
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Copy the built binary
COPY --from=builder /app/target/release/${PACKAGE} /usr/local/bin/app

ENV RUST_LOG=info \
    RUST_BACKTRACE=1

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/app"]
