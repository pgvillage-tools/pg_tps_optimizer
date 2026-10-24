# Base images are pinned to a version and digest for reproducible builds; Dependabot updates them
FROM rust:1.99-trixie@sha256:931c2c0531a8b143e044f0f3914dfe802f15bb3d1955f0b657e9359327ac2ffc AS builder
WORKDIR /usr/src/app

COPY . .
RUN cargo install --path .

# Same Debian release as the builder image, for a matching glibc. OpenSSL is compiled into the
# binary (vendored); ca-certificates provides the CAs of the OS for sslrootcert=system.
FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/pg_tps_optimizer /usr/local/bin/pg_tps_optimizer

COPY README.md LICENSE .
ENTRYPOINT [ "/usr/local/bin/pg_tps_optimizer" ]
