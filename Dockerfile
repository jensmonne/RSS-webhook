FROM rust:1.80-slim-bullseye AS builder

# Install system dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

# 1. Create a dummy project to cache dependencies
COPY Cargo.toml .
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release

# 2. Now copy your actual source code
COPY src ./src
# This touch ensures Cargo sees the file as "new"
RUN touch src/main.rs
RUN cargo build --release

# -- Runtime --
FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/rsswebhook /usr/local/bin/rsswebhook
WORKDIR /data
CMD ["rsswebhook"]
