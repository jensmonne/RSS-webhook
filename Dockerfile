FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -f target/release/deps/rsswebhook*

COPY src ./src
RUN cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates
COPY --from=builder /usr/src/app/target/release/rsswebhook /usr/local/bin/rsswebhook
WORKDIR /data
CMD ["rsswebhook"]
