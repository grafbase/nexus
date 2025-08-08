FROM rust:1.89-bookworm AS chef

COPY rust-toolchain.toml rust-toolchain.toml
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
    cargo binstall --no-confirm cargo-chef cargo-zigbuild sccache
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
    cargo chef cook --release --recipe-path recipe.json --zigbuild

COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
COPY ./crates ./crates
COPY ./nexus ./nexus

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo zigbuild --release --bin nexus

#
# === Final image ===
#
FROM debian:bookworm-slim

LABEL org.opencontainers.image.url='https://nexusrouter.com' \
    org.opencontainers.image.documentation='https://nexusrouter.com/docs' \
    org.opencontainers.image.source='https://github.com/grafbase/nexus' \
    org.opencontainers.image.vendor='Grafbase' \
    org.opencontainers.image.description='The Grafbase AI Router' \
    org.opencontainers.image.licenses='MPL-2.0'

WORKDIR /nexus

# used curl to run a health check query against the server in a docker-compose file
RUN apt update && apt upgrade -y && apt install -y curl && rm -rf /var/lib/apt/lists/*

RUN adduser -u 1000 --home /data nexus && mkdir -p /data && chown nexus /data
COPY --from=builder /nexus/crates/config/examples/nexus.toml /etc/nexus.toml
USER nexus

COPY --from=builder /nexus/target/release/nexus /bin/nexus

VOLUME /data
WORKDIR /data

ENTRYPOINT ["/bin/nexus"]
CMD ["--config", "/etc/nexus.toml", "--listen-address", "0.0.0.0:3000"]

EXPOSE 3000
