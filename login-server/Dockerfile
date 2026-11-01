# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo test --release && cargo build --release --bins \
    && cp target/release/login-server /login-server \
    && cp target/release/login-bench /login-bench

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends libssl3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /login-server /usr/local/bin/login-server
COPY --from=builder /login-bench /usr/local/bin/login-bench
COPY config.toml.example /opt/login-server/config.toml.example
EXPOSE 7173
ENTRYPOINT ["login-server"]
