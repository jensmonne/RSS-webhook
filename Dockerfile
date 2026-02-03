FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -f target/release/deps/rsswebhook*

COPY src ./src
RUN cargo build --release
RUN strip target/release/rsswebhook

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/rsswebhook /usr/local/bin/rsswebhook
WORKDIR /data
CMD ["rsswebhook"]
