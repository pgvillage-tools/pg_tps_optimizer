# Testing

## Unit tests

```
cargo test
```

This runs the unit tests, and the tests in `tests/cli.rs`, which run the binary without
PostgreSQL. The end-to-end tests are skipped (shown as `ignored`), as they need a container
runtime.

## End-to-end tests

The end-to-end tests in `tests/e2e.rs` start PostgreSQL in containers with
[testcontainers](https://github.com/testcontainers/testcontainers-rs), and run the
`pg_tps_optimizer` binary against them:

```
cargo test --test e2e -- --ignored
# or
make integrationtest
```

They need docker, or podman with its docker socket (`podman machine start` on macOS). Every test
starts its own PostgreSQL container, and removes it when it finishes.

- `PGVERSION` selects the PostgreSQL version (the `postgres` image tag), default `18`:
  `PGVERSION=14 cargo test --test e2e -- --ignored`
- The tests generate their own certificates (CA, server and client) with `rcgen`.
- The binary runs with only the `PG*` environment variables of the test, and `HOME` set to an empty
  directory, so your own settings and `~/.postgresql` don't influence the tests.

The tests cover every `sslmode` against a server with and without TLS (see the
[connection behaviour](../users-guide/connecting.md#connection-behaviour)), client certificates,
connection strings, environment variables and service files.

## Linting

CI fails on any `rustfmt` difference or `clippy` warning:

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Documentation

The documentation is built with [MkDocs Material](https://squidfunk.github.io/mkdocs-material/)
from `docs/`. The navigation is in `docs/SUMMARY.md`. To build and view it locally:

```
python3 -m venv .venv
.venv/bin/pip install -r docs-requirements.txt
DISABLE_MKDOCS_2_WARNING=true .venv/bin/mkdocs serve
```

and open <http://127.0.0.1:8000>. CI builds the docs with `mkdocs build --strict`, which fails on
broken links.

## CI

Every pull request runs `.github/workflows/ci-run-on-pr.yaml`. To save resources, jobs only run when
files they depend on changed:

| Job | Runs when |
|---|---|
| Rustfmt and Clippy, Run unit tests | Rust code, `Cargo.toml`/`Cargo.lock` or tests changed |
| Run e2e tests (PostgreSQL 14 ... 18) | the same, after the unit tests passed |
| Build docker image (amd64, arm64) | `Containerfile`, `.dockerignore`, `Cargo.toml` or `Cargo.lock` changed |
| Build documentation | `docs/`, `mkdocs.yml`, `mkdocs_overrides/` or `docs-requirements.txt` changed |

When the workflow itself changes, all jobs run. Draft pull requests are skipped.
