# -- Builder Stage --
FROM rust:slim-bullseye as builder

WORKDIR /usr/src/app

# Install OpenSSL development packages (Required for reqwest/https)
RUN apt-get update && apt-get install -y pkg-config libssl-dev

# Copy your source code
COPY . .

# Build the binary in release mode
RUN cargo build --release

# -- Runtime Stage --
# We use a slim Debian image for the final container to keep it small
FROM debian:bullseye-slim

# Install OpenSSL runtime libraries and CA certificates
# ca-certificates is CRITICAL for https requests to work
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/rsswebhook /usr/local/bin/rsswebhook

# Set the working directory to where we will map the volume
WORKDIR /data

# Run the binary
CMD ["rsswebhook"]
