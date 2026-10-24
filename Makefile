build:
	./set_version.sh
	cargo build

run:
	cargo run

test: unittest coverage

unittest:
	cargo test

# Needs docker (or podman with its docker socket)
# Every e2e test runs a benchmark at full load against its own container, so limit how many run at
# the same time (the default is one per CPU core of the host, not of the docker/podman VM)
E2E_THREADS ?= 8
integrationtest:
	cargo test --test e2e -- --ignored --test-threads=$(E2E_THREADS)

coverage:
	./unittests.sh

sectest: audittest

audittest:
	cargo audit

checkmate:
	cargo checkmate

.PHONY: build-linux build-darwin build-all

# Cross builds; linux targets need `cargo install cross` and a running docker,
# darwin targets need to be built on macOS (`rustup target add <target>`)
LINUX_TARGETS = x86_64-unknown-linux-musl aarch64-unknown-linux-musl
DARWIN_TARGETS = x86_64-apple-darwin aarch64-apple-darwin

build-linux:
	for target in $(LINUX_TARGETS); do cross build --release --target $$target || exit 1; done

build-darwin:
	for target in $(DARWIN_TARGETS); do cargo build --release --target $$target || exit 1; done

build-all: build-linux build-darwin
