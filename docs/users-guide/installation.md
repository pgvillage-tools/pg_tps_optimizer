# Installation

pg_tps_optimizer is a single binary. You can download it, use the container image, or build it
yourself.

## Binary

Every [release](https://github.com/pgvillage-tools/pg_tps_optimizer/releases) has an archive per
platform, with a `.sha256` checksum file next to it:

| Platform | Archive |
|---|---|
| Linux, x86_64 (amd64) | `pg_tps_optimizer-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Linux, ARM64 | `pg_tps_optimizer-<version>-aarch64-unknown-linux-musl.tar.gz` |
| macOS, Intel | `pg_tps_optimizer-<version>-x86_64-apple-darwin.tar.gz` |
| macOS, Apple silicon | `pg_tps_optimizer-<version>-aarch64-apple-darwin.tar.gz` |

The Linux binaries are statically linked (musl), so they run on any Linux distribution. OpenSSL is
built into all binaries; you don't need to install it.

```
VERSION=0.3.0
TARGET=x86_64-unknown-linux-musl
NAME=pg_tps_optimizer-${VERSION}-${TARGET}
curl -fLO https://github.com/pgvillage-tools/pg_tps_optimizer/releases/download/v${VERSION}/${NAME}.tar.gz
curl -fLO https://github.com/pgvillage-tools/pg_tps_optimizer/releases/download/v${VERSION}/${NAME}.tar.gz.sha256
shasum -a 256 -c ${NAME}.tar.gz.sha256
tar -xzf ${NAME}.tar.gz
./${NAME}/pg_tps_optimizer --version
```

!!! note
    Releases up to v0.2.7 only have a Linux x86_64 build, as a zip file.

## Container image

The image `ghcr.io/pgvillage-tools/pg_tps_optimizer` on the GitHub Container Registry runs
pg_tps_optimizer as its entrypoint, for `linux/amd64` and `linux/arm64`. Every release is tagged
with its version (like `v0.3.1`), and the newest release with `latest`. Arguments after the image
name are passed to pg_tps_optimizer, and settings can be passed as environment variables (one `-e`
per variable):

```
docker run --rm \
  -e PGHOST=db1.example.com -e PGUSER=postgres -e PGPASSWORD=secret \
  ghcr.io/pgvillage-tools/pg_tps_optimizer --range 200
```

!!! note
    Releases up to v0.2.7 were published on Docker Hub, as `mannemsolutions/pg_tps_optimizer`
    (only for `linux/amd64`).

To use certificates, mount them into the container; see
[Connecting to PostgreSQL](connecting.md#example-client-certificate-and-verify-full).

## Build from source

With a [Rust toolchain](https://rustup.rs/):

```
cargo install --git https://github.com/pgvillage-tools/pg_tps_optimizer
```

See [Building](../developers-guide/building.md) for more options, like cross-compiling.
