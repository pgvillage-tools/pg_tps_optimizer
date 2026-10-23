# Building

pg_tps_optimizer is written in Rust. You need a [Rust toolchain](https://rustup.rs/) (stable), and
for OpenSSL, which is compiled into the binary, `perl` and `make`.

```
git clone https://github.com/pgvillage-tools/pg_tps_optimizer
cd pg_tps_optimizer
cargo build --release
./target/release/pg_tps_optimizer --help
```

## Make targets

| Target | What it does |
|---|---|
| `make build` | Sets the version in `Cargo.toml` from the latest git tag (`set_version.sh`), then `cargo build` |
| `make run` | `cargo run` |
| `make unittest` | `cargo test` |
| `make integrationtest` | The e2e tests, see [Testing](testing.md) |
| `make coverage` | Unit tests with code coverage (`unittests.sh`, needs `llvm-tools`) |
| `make audittest` | `cargo audit`: known vulnerabilities in dependencies |
| `make build-linux` | Release builds for Linux x86_64 and ARM64 (musl), with [cross](https://github.com/cross-rs/cross) |
| `make build-darwin` | Release builds for macOS Intel and Apple silicon (on macOS) |
| `make build-all` | `build-linux` and `build-darwin` |

## Cross builds

The release workflow builds these targets:

| Target | How |
|---|---|
| `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | `cross build --release --target <target>`: needs `cargo install cross` and docker (or podman) |
| `x86_64-apple-darwin`, `aarch64-apple-darwin` | `cargo build --release --target <target>` on macOS, after `rustup target add <target>` |

## Container image

The image is built from `Containerfile`:

```
docker build -f Containerfile -t pg_tps_optimizer .
# or
podman build -f Containerfile -t pg_tps_optimizer .
```

The base images are pinned to a version and digest; Dependabot updates them. `.dockerignore` keeps
`target/` out of the build context.

## Version

The version in `Cargo.toml` is set from the git tag when building a release (`set_version.sh`):
tag `v1.2.3` gives version `1.2.3`. Outside a release, it's set from the latest `v*` tag.
