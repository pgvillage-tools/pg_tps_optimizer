FROM rust:latest AS builder
WORKDIR /usr/src/app

COPY . .
RUN cargo install --path .

FROM debian:latest
RUN /bin/sh -c set -eux; apt-get update; apt-get install -y openssl ; rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/pg_tps_optimizer /usr/local/bin/pg_tps_optimizer

COPY README.md LICENSE .
ENTRYPOINT [ "/usr/local/bin/pg_tps_optimizer" ]
