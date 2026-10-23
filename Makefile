build:
	./set_version.sh
	cargo build

run:
	cargo run

test: unittest coverage

unittest:
	cargo test

integrationtest:
	./docker-compose-tests.sh

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
