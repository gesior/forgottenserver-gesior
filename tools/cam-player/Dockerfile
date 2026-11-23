# Stage 1: Build static Rust binary
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml ./
# Create dummy main to pre-cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy real sources
COPY src ./src

# Build release binary
RUN touch src/main.rs && cargo build --release

# Stage 2: Minimal runtime image
FROM alpine:latest

RUN apk add --no-cache 7zip ca-certificates tzdata

WORKDIR /srv

COPY --from=builder /app/target/release/cam-player /usr/local/bin/cam-player
COPY config.json /srv/config.json

ENV LISTEN_ADDR=0.0.0.0:7173
ENV CAMS_DIR=/srv/camsReady/
ENV RSA_KEY_PATH=/srv/key.pem
ENV CACHE_DIR=/srv/cache/
ENV DB_HOST=127.0.0.1
ENV DB_PORT=3306
ENV DB_NAME=forgottenserver
ENV DB_USER=forgottenserver
ENV DB_PASS=

EXPOSE 7173

CMD ["cam-player"]
