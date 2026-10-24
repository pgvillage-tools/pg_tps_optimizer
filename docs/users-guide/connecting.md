# Connecting to PostgreSQL

This page describes how pg_tps_optimizer connects to PostgreSQL: where it gets its connection
settings from, how TLS (`sslmode`) works, and what happens when connecting fails.

pg_tps_optimizer follows the conventions of libpq (the library `psql` uses) where it can, so most
settings work the way you know from `psql`. Differences are listed explicitly below.

## Connection settings

Connection settings come from four places. Later sources override earlier ones, like in libpq:

1. Built-in defaults
2. Environment variables
3. A service from a [service file](#service-files), when `service` or `PGSERVICE` is set
4. The connection string, given with `--dsn` (`-d`) or the `PGTPSSOURCE` environment variable
   (`--dsn` wins when both are set)

| Setting | Environment variable | Default |
|---|---|---|
| `host` | `PGHOST` | `hostaddr` when that is set; otherwise the unix socket directory with a running server for `port`: `/var/run/postgresql` or `/tmp` (`/tmp` when neither has one) |
| `hostaddr` | `PGHOSTADDR` | not set |
| `port` | `PGPORT` | `5432` |
| `user` | `PGUSER` | the current OS user |
| `dbname` | `PGDATABASE` | the user name |
| `password` | `PGPASSWORD` | from the [password file](#password-file), if a line matches |
| `passfile` | `PGPASSFILE` | `~/.pgpass` |
| `sslmode` | `PGSSLMODE` | `prefer` |
| `sslcert` | `PGSSLCERT` | `~/.postgresql/postgresql.crt`, if it exists |
| `sslkey` | `PGSSLKEY` | `~/.postgresql/postgresql.key`, if it exists |
| `sslrootcert` | `PGSSLROOTCERT` | `~/.postgresql/root.crt`, if it exists; `system` for the CAs of the operating system |
| `application_name` | `PGAPPNAME` | not set |
| `options` | `PGOPTIONS` | not set |
| `connect_timeout` | `PGCONNECT_TIMEOUT` | no timeout |
| `target_session_attrs` | `PGTARGETSESSIONATTRS` | `any` (also `read-write`, `read-only`) |
| `channel_binding` | `PGCHANNELBINDING` | `prefer` |
| `load_balance_hosts` | `PGLOADBALANCEHOSTS` | `disable` |
| `sslnegotiation` | `PGSSLNEGOTIATION` | `postgres` |

Other settings the PostgreSQL driver supports (`tcp_user_timeout`, `keepalives`, `keepalives_idle`,
`keepalives_interval`, `keepalives_retries`) can be set in the connection string; libpq has no
environment variables for them either.

### Connection string format

The connection string uses the same formats as libpq: `key=value` pairs, or a URI.

```
pg_tps_optimizer --dsn 'host=db1.example.com port=5432 user=postgres dbname=postgres sslmode=verify-full'
pg_tps_optimizer --dsn 'postgresql://postgres@db1.example.com:5432/postgres?sslmode=verify-full'
```

`key=value` pairs:

- Are separated by whitespace; whitespace around `=` is allowed.
- Values with spaces must be quoted with single quotes: `application_name='pg tps'`. Within a
  value, a backslash escapes the next character: `'it\'s'`.
- An empty value must be quoted as well (`password=''`): `password= user=x` sets the password to
  `user=x`, like in libpq.

URIs (`postgresql://` or `postgres://`):

- `postgresql://[user[:password]@][host[:port][,host[:port]...]][/dbname][?key=value[&...]]`
- Every part is optional, and `?key=value` accepts any connection setting, like
  `?sslmode=verify-full&sslrootcert=/etc/pg/root.crt`.
- IPv6 addresses go in brackets (`[::1]:5432`), and special characters must be percent-encoded,
  e.g. `%40` for `@` in a password, or `%2Fvar%2Frun%2Fpostgresql` for a unix socket directory as
  host.

In both formats, `~` is not expanded (only in environment variables): use absolute paths for
certificate files.

### Service files

A service file holds named sets of connection settings, so you only need to give the service
name, with `service=bench` in the connection string or `PGSERVICE=bench`:

```
# ~/.pg_service.conf
[bench]
host=db1.example.com
port=5432
user=postgres
dbname=postgres
sslmode=verify-full
sslrootcert=/etc/pg_tps_optimizer/root.crt
```

The service is looked up like libpq does, in the first of these files that defines it:

1. The user service file: `PGSERVICEFILE`, or `~/.pg_service.conf`
2. The system service file: `pg_service.conf` in `PGSYSCONFDIR`. Without `PGSYSCONFDIR`,
   `/etc/postgresql-common/pg_service.conf` (Debian/Ubuntu) and
   `/etc/sysconfig/pgsql/pg_service.conf` (RHEL) are used. (libpq uses the one directory it was
   built with.)

Every line in a service is `key=value`; values are not quoted, and lines starting with `#` are
comments. Settings in the connection string override the service, and the service overrides
environment variables. When the service isn't defined in any of the files, pg_tps_optimizer stops
with `definition of service "bench" not found`.

### Password file

When no password is set (with `password=`, in a service file or with `PGPASSWORD`), the password
is looked up in the password file, like libpq does: `passfile` in the connection string,
`PGPASSFILE`, or `~/.pgpass`.

```
# ~/.pgpass: hostname:port:database:username:password
db1.example.com:5432:postgres:postgres:secret
*:*:*:benchmark:other\:secret
```

- The first line that matches the connection wins; `*` matches anything.
- A `\` escapes the next character, so `\:` is a `:` and `\\` is a `\`.
- A unix socket connection matches the host name `localhost`.
- With several hosts, the first host and port are used.
- The file must only be readable by you (`chmod 600 ~/.pgpass`). A file that others can read is
  ignored, with the warning `password file "..." has group or world access`.

### Not supported

- Certificate revocation lists (`sslcrl`, `PGSSLCRL`). An `sslcrl` in the connection string is
  ignored. Please open an issue if you need it.

## TLS and sslmode

`sslmode` decides whether TLS is used, and how much of the server certificate is checked:

| sslmode | Uses TLS | Checks the server certificate | Server without TLS |
|---|---|---|---|
| `disable` | never | – | connects |
| `allow` | only when connecting without TLS fails | no | connects |
| `prefer` (default) | when the server supports it | no | connects |
| `require` | always | no | fails |
| `verify-ca` | always | signed by the root certificate | fails |
| `verify-full` | always | signed by the root certificate, and valid for the host name | fails |

Some things to keep in mind:

- **Use `verify-full` to protect against a man-in-the-middle.** `prefer` and `require` encrypt the
  connection, but don't check who you are talking to. `verify-ca` checks that the server
  certificate is signed by your root certificate, but not for which server: any server with a
  certificate from that CA is accepted. That is only safe when the CA signs nothing but your
  PostgreSQL servers. `verify-full` also checks the host name.
- **Only `sslrootcert` is trusted.** `verify-ca` and `verify-full` don't trust the CAs of the
  operating system, only the root certificate(s) in `sslrootcert` (or `~/.postgresql/root.crt`),
  unless `sslrootcert` is `system` (see below).
- **`sslrootcert=system`** trusts the CAs of the operating system instead of a root certificate
  file, like in libpq 16 and newer. Use it for a server with a certificate from a public CA.
    - Any public CA can issue a certificate, so it requires `verify-full`: that's the default with
      `sslrootcert=system`, and a weaker `sslmode` gives an error.
    - When `SSL_CERT_FILE` (a file) or `SSL_CERT_DIR` (directories, separated by `:`) is set,
      only those CAs are trusted, and they must exist. Otherwise the CAs come from the usual
      locations, like `/etc/ssl/certs`, `/etc/pki/tls` or `/etc/ssl/cert.pem` (macOS).
    - The container image has no CA certificates: mount them and set `SSL_CERT_FILE`.
- **`require` never checks the server certificate**, not even when a root certificate is configured.
  This differs from libpq, where `require` behaves like `verify-ca` when a root certificate file
  exists.
- **`verify-ca` and `verify-full` need a root certificate.** Without one (no `sslrootcert`, and no
  `~/.postgresql/root.crt`) pg_tps_optimizer stops with an error, before connecting.
- **`verify-full` checks the host name you connect to** (`host`) against the server certificate.
  Connecting to `127.0.0.1` with a certificate that is only valid for `localhost` fails.
- **`allow`** first connects without TLS. Only when that fails (for instance because `pg_hba.conf`
  only accepts TLS connections) does it try again with TLS. When both fail, the error of the TLS
  attempt is reported.
- **Unix sockets never use TLS.** Like libpq, `sslmode` is ignored when `host` is a directory
  (starts with `/`). A host list that mixes unix sockets and TCP hosts (like
  `host=/var/run/postgresql,db1.example.com`) can't be used with `require`, `verify-ca` or
  `verify-full`: pg_tps_optimizer stops with an error, instead of connecting to the TCP hosts
  without TLS.

### Client certificates

A client certificate is used when `sslcert` is set (or `~/.postgresql/postgresql.crt` exists), with
the private key from `sslkey` (or `~/.postgresql/postgresql.key`). This is needed when the server
uses `cert` authentication in `pg_hba.conf`. The user name must match the common name (CN) of the
certificate.

Certificates and keys must be PEM files. When a certificate, key or root certificate is set but
can't be loaded (missing file, wrong format), pg_tps_optimizer stops with an error, instead of
connecting without it. A certificate and key only work as a pair: setting only one of them (or
having only one of the two default files) is an error as well. libpq silently ignores a key
without a certificate.

### Example: client certificate and verify-full

```
export PGHOST=db1.example.com
export PGUSER=postgres
export PGDATABASE=postgres
export PGSSLMODE=verify-full
export PGSSLROOTCERT=/etc/pg_tps_optimizer/root.crt
export PGSSLCERT=/etc/pg_tps_optimizer/postgres.crt
export PGSSLKEY=/etc/pg_tps_optimizer/postgres.key
pg_tps_optimizer
```

With the container image, pass the same environment variables (one `-e` per variable) and mount
the certificates:

```
docker run --rm \
  -e PGHOST=db1.example.com -e PGUSER=postgres -e PGDATABASE=postgres \
  -e PGSSLMODE=verify-full \
  -e PGSSLROOTCERT=/certs/root.crt -e PGSSLCERT=/certs/postgres.crt -e PGSSLKEY=/certs/postgres.key \
  -v /etc/pg_tps_optimizer:/certs:ro \
  mannemsolutions/pg_tps_optimizer
```

## Connection behaviour

This table shows the result of every `sslmode` for different server setups. *Connects* means the
benchmark runs; *fails* means pg_tps_optimizer stops with an error (see
[When connecting fails](#when-connecting-fails)).

| Server | Root certificate | Host | disable | allow | prefer | require | verify-ca | verify-full |
|---|---|---|---|---|---|---|---|---|
| without TLS | correct | localhost | ✅ connects | ✅ connects | ✅ connects | ❌ fails | ❌ fails | ❌ fails |
| TLS only | correct | localhost | ❌ fails | ✅ connects | ✅ connects | ✅ connects | ✅ connects | ✅ connects |
| TLS only | wrong CA | localhost | | | ✅ connects | ✅ connects ¹ | ❌ fails | ❌ fails |
| TLS only | none | localhost | | | | ✅ connects | ❌ fails | ❌ fails |
| TLS only | correct | 127.0.0.1 ² | | | | | ✅ connects | ❌ fails |
| TLS only | wrong CA, but trusted by the OS ³ | localhost | | | | | ❌ fails | ❌ fails |
| without TLS | correct | `/nonexistent,localhost` ⁴ | | | ✅ connects | ❌ fails | | |

- **Server without TLS:** `ssl = off` in `postgresql.conf`.
- **TLS only:** `ssl = on`, and `pg_hba.conf` only has `hostssl` entries (no `host` or `hostnossl`),
  so connections without TLS are rejected.
- **Root certificate:** *correct* is the CA that signed the server certificate, *wrong CA* is a CA
  that didn't, and *none* means no `sslrootcert` and no `~/.postgresql/root.crt`.
- Empty cells are not tested; they behave like the *correct* root certificate row.

¹ `require` never checks the server certificate. libpq checks the CA for `require` when a root
certificate is configured; pg_tps_optimizer deliberately doesn't.
² The server certificate is only valid for `localhost`, so only `verify-full` notices the mismatch.
³ `sslrootcert` doesn't contain the CA that signed the server certificate, but the trust store of
the operating system does. Only `sslrootcert` is trusted.
⁴ A host list with a unix socket and a TCP host. `require`, `verify-ca` and `verify-full` are refused
for such a list, instead of connecting to the TCP host without TLS.

Every filled cell is covered by an end-to-end test in [`tests/e2e/sslmode.rs`](https://github.com/pgvillage-tools/pg_tps_optimizer/blob/main/tests/e2e/sslmode.rs), which runs
pg_tps_optimizer against PostgreSQL in a container.

## When connecting fails

When pg_tps_optimizer can't connect, it prints a single line starting with `error:` to stderr and
exits with exit code 2, 3 or 4 (see below). The line includes the reason PostgreSQL or the TLS library gave, for
example:

```
error: connection to server at "localhost", port 5432 failed: error performing TLS handshake: server does not support TLS
error: root certificate file "~/.postgresql/root.crt" does not exist; either provide the file or change sslmode to disable server certificate verification
error: connection to server at "localhost", port 5432 failed: error connecting to server: Connection refused (os error 61)
error: connection to server at "localhost", port 5432 failed: db error: FATAL: database "nope" does not exist
```

| Message contains | Likely cause |
|---|---|
| `server does not support TLS` | `sslmode` is `require` or `verify-*`, but `ssl = off` on the server |
| `no pg_hba.conf entry ... no encryption` | the server only accepts TLS, but `sslmode` is `disable` |
| `connection requires a valid client certificate` | `pg_hba.conf` uses `cert`, but no client certificate is configured |
| `certificate verify failed` | `sslmode=verify-*` and the server certificate isn't signed by the root certificate, or (`verify-full`) isn't valid for the host name |
| `root certificate file ... does not exist` | `sslmode=verify-*` without a root certificate |
| `weak sslmode ... may not be used with sslrootcert=system` | `sslrootcert=system` with an `sslmode` other than `verify-full` |
| `SSL_CERT_FILE ... does not exist` (or `SSL_CERT_DIR`) | `sslrootcert=system` with an `SSL_CERT_FILE` or `SSL_CERT_DIR` that doesn't exist |
| `sslrootcert=system: no certificates of the OS found` | no CA certificates in the usual locations (for instance in the container): set `SSL_CERT_FILE` or `SSL_CERT_DIR` |
| `sslkey is set, but sslcert is not` (or the other way around) | only one of the client certificate and key is configured, or only one of the default files in `~/.postgresql` exists |
| `mixes unix sockets and TCP hosts` | `host` lists unix sockets and TCP hosts, with `sslmode` `require` or `verify-*` |
| `definition of service ... not found` | `service` or `PGSERVICE` names a service that is not in any [service file](#service-files) |
| `invalid connection string` | the connection string can't be parsed (for instance an unterminated quote, or a key without `=`), or has an unknown key (for instance `client_encoding`) |
| `password authentication failed` | wrong or missing password: check `PGPASSWORD`, `password=` or the [password file](#password-file) (and its permissions) |
| `Connection refused` | wrong host or port, or PostgreSQL isn't running |

The exit code tells the kind of error: 2 for invalid connection settings, 3 when the server can't be
reached or TLS fails, and 4 when the server refuses the connection; see
[Exit codes](usage.md#exit-codes).

When a connection of one of the benchmark clients fails during a run, the client logs the error and
tries to reconnect, without stopping the run.
