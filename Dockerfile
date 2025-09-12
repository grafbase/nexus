FROM rust:1.89-slim AS chef

# Install build dependencies with retry logic for flaky networks
RUN apt-get update && apt-get install -y \
    curl \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY rust-toolchain.toml rust-toolchain.toml
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
    cargo binstall --no-confirm cargo-chef sccache
ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache

WORKDIR /nexus

FROM chef AS planner
# At this stage we don't really bother selecting anything specific, it's fast enough.
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ENV CARGO_INCREMENTAL=0
COPY --from=planner /nexus/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
COPY ./crates ./crates
COPY ./nexus ./nexus

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo build --release --bin nexus

#
# === Final image ===
#
FROM cgr.dev/chainguard/wolfi-base:latest

LABEL org.opencontainers.image.url='https://nexusrouter.com' \
    org.opencontainers.image.documentation='https://nexusrouter.com/docs' \
    org.opencontainers.image.source='https://github.com/grafbase/nexus' \
    org.opencontainers.image.vendor='Grafbase' \
    org.opencontainers.image.description='The Grafbase AI Router' \
    org.opencontainers.image.licenses='MPL-2.0'

WORKDIR /nexus

# Install curl for health checks
RUN apk add --no-cache curl

# Create user and directories
# wolfi-base uses adduser from busybox
RUN adduser -D -u 1000 nexus && mkdir -p /data && chown nexus:nexus /data
COPY --from=builder /nexus/crates/config/examples/nexus.toml /etc/nexus.toml
USER nexus

COPY --from=builder /nexus/target/release/nexus /bin/nexus

VOLUME /data
WORKDIR /data

ENTRYPOINT ["/bin/nexus"]
CMD ["--config", "/etc/nexus.toml", "--listen-address", "0.0.0.0:3000"]

EXPOSE 3000
