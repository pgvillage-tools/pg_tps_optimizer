//! End-to-end tests: run the pg_tps_optimizer binary against PostgreSQL in a container.
//!
//! These tests need a container runtime (docker, or podman with its docker socket) and are
//! ignored by default. Run them with:
//!
//!     cargo test --test e2e -- --ignored
//!
//! Set PGVERSION to test against another PostgreSQL major version (default: 18).
//!
//! The sslmode tests describe the expected behavior for every sslmode, against a server without
//! TLS and a server that only accepts TLS connections. Expectations follow libpq (psql), see
//! https://www.postgresql.org/docs/current/libpq-ssl.html#LIBPQ-SSL-PROTECTION
//!
//! The tests are split per subject; add tests for a new feature in its own module, so parallel
//! changes don't touch the same file:
//! - common: helpers (certificates, PostgreSQL containers, running the binary)
//! - sslmode: sslmode, TLS and certificates
//! - connection: connection strings, environment variables, service files
//! - workload: transactional and prepared modes

mod common;
mod connection;
mod sslmode;
mod workload;
