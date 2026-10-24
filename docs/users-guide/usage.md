# Usage

```
pg_tps_optimizer [OPTIONS]
```

Without options, pg_tps_optimizer connects with the default
[connection settings](connecting.md) and runs with 1 up to 987 clients.

## Options

Every option can also be set with an environment variable, or in a [config file](#config-file). An
option on the command line wins over the environment variable, which wins over the config file,
which wins over the default.

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `-d`, `--dsn` | `PGTPSSOURCE` | | Connection string; see [Connecting to PostgreSQL](connecting.md) |
| `-r`, `--range` | `PGTPSRANGE` | `1:1000` | The number of clients to run with: `min:max`, or only `max` |
| `-m`, `--min-samples` | `PGTPSMINSAMPLES` | `10` | The number of samples (of 200ms) to check if a step is stable |
| `-s`, `--spread` | `PGTPSSPREAD` | `10` | The maximum spread (in %) of TPS and latency for a stable step |
| `-M`, `--max-wait` | `PGTPSMAXWAIT` | `10s` | How long to wait for a step to become stable, like `10s`, `1m30s` or `20` (seconds) |
| `-t`, `--transactional` | `PGTPSTRANSACTIONAL` | off | Run every query in its own transaction (`begin` / `commit`) |
| `-p`, `--prepared` | `PGTPSPREPARED` | off | Run the query as a prepared statement |
| `-q`, `--query` | `PGTPSQUERY` | | The query every client runs, with the number of the client as `$1`; see [The workload](#the-workload) |
| `-o`, `--output` | `PGTPSOUTPUT` | `table` | `table`: a table while running; `json`: a JSON document at the end, see [JSON output](results.md#json-output) |
| `-g`, `--graph` | `PGTPSGRAPH` | | Write a graph of the TPS per number of clients to this file, see [Graph](results.md#graph) |
| `--graph-format` | `PGTPSGRAPHFORMAT` | `auto` | `png`, `svg`, or `auto`: SVG when the file name ends with `.svg`, otherwise PNG |
| `--graph-input` | `PGTPSGRAPHINPUT` | | Don't run, but draw the graph of these JSON reports (from `--output json`); repeat it, or separate the files with commas |
| `-c`, `--config` | `PGTPSCONFIG` | | A YAML [config file](#config-file) with the options |
| `-h`, `--help` | | | Show the options |
| `-V`, `--version` | | | Show the version |

For the on/off options, the environment variable can be `true`/`false`, `1`/`0`, `yes`/`no` or
`on`/`off`.

## Config file

Instead of options, you can put the settings in a YAML file, and pass it with `--config` (or
`PGTPSCONFIG`):

```yaml
# pg_tps_optimizer.yaml
dsn: host=db1.example.com user=postgres dbname=postgres sslmode=verify-full
range: "1:200"
min-samples: 20
spread: 5
max-wait: 30s
prepared: true
```

```
pg_tps_optimizer --config pg_tps_optimizer.yaml
```

- Every option can be set, with its long name as key: `min-samples` (or `min_samples`), `max-wait`,
  and so on. Options that take more values take a list: `graph-input: [a.json, b.json]`.
- The values in the config file replace the defaults: `--help` shows them as defaults, and an option
  on the command line or an environment variable still wins. So you can keep the common settings in
  the file, and change one with an option: `pg_tps_optimizer -c pg_tps_optimizer.yaml --range 8:200`.
- Quote values that YAML could read differently, like `range: "1:200"`.
- An unknown key, invalid YAML or an invalid value stops pg_tps_optimizer with an error (exit code 2).

## How it works

1. pg_tps_optimizer starts with the lowest number of clients from `--range`.
2. Every client runs the workload as fast as it can. Every 200ms, the clients report how many
   transactions they ran, and how long they waited for them. Such a period of 200ms is a sample.
3. When there are `--min-samples` samples, pg_tps_optimizer checks if the step is stable: the
   standard deviation of both TPS and latency must be below `--spread` percent of their mean.
4. When the step is stable, or after `--max-wait`, it prints the results of the step, and moves on
   to the next number of clients in the Fibonacci sequence: 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, ...
   A step that didn't become stable is marked with a `*`.
5. It stops before the number of clients reaches the maximum of `--range`.

Because of the Fibonacci steps, the maximum works in steps as well: with `--range 100` and
`--range 99`, the last step is 89 clients, and with `--range 200` it is 144.

### The workload

Every client runs a query as fast as it can: the default workload, or your own query.

**The default workload.** pg_tps_optimizer creates a table `pg_tps_optimizer (id oid)` in the
database it connects to (if it doesn't exist yet), and empties it. Every client inserts a row with
its own number, and then runs:

```sql
update pg_tps_optimizer set id = $1 where id = $1
```

So the user needs permission to create the table (or the table must exist), and to insert and
update it. After the run, the table is dropped. (If the run fails halfway, it's left behind.)

**Your own query.** With `--query` (or `PGTPSQUERY`, or `query:` in the config file), every client
runs your query instead, and no table is created:

```
pg_tps_optimizer --query 'select * from accounts where id = $1::int'
pg_tps_optimizer --query 'insert into events (client) values ($1)'
```

- `$1` is the number of the client (0, 1, 2, ...), passed as the type the query needs for it: a
  number (`int`, `bigint`, `oid`, ...) or `text`. When PostgreSQL can't tell the type, cast it,
  like `$1::int`. The query doesn't need to use `$1`.
- Only `$1` is supported; a query with `$2` or more gives an error.
- The query is checked before the clients start, so a typo stops pg_tps_optimizer with
  `error: invalid query: ...` (exit code 2).
- Rows a query returns are read, but not used.

With `--transactional`, every query runs in its own transaction (`begin`, query, `commit`). With
`--prepared`, every client prepares the query once (again after a reconnect), and then only
executes it, which saves parsing and planning it for every transaction. Both can be combined.

## Examples

Run up to 144 clients, check 20 samples (4 seconds) per step, accept a spread of 5%, and wait at
most 20 seconds per step:

```
pg_tps_optimizer --dsn 'host=db1.example.com user=postgres dbname=postgres' \
  --range 200 --min-samples 20 --spread 5 --max-wait 20s
```

The same with environment variables, for instance in a container:

```
docker run --rm \
  -e PGHOST=db1.example.com -e PGUSER=postgres -e PGDATABASE=postgres -e PGPASSWORD=secret \
  -e PGTPSRANGE=200 -e PGTPSMINSAMPLES=20 -e PGTPSSPREAD=5 -e PGTPSMAXWAIT=20s \
  mannemsolutions/pg_tps_optimizer
```

Start at 8 clients, for a server you know can handle that:

```
pg_tps_optimizer --range 8:1000
```

## Exit codes

Errors are printed to stderr as one line, starting with `error:`. The exit code tells what kind of
error it was, so scripts can react to it:

| Exit code | Meaning | Examples |
|---|---|---|
| 0 | The run finished | |
| 1 | An error during the run | a query fails after connecting |
| 2 | Invalid options, environment variables, config file or connection settings: fix the command | `--spread abc`, an invalid `--max-wait` or `--query`, an unknown key in the config file, an invalid connection string or `sslmode`, a missing certificate file, an unknown service |
| 3 | The server can't be reached, or TLS fails | a wrong host or port, `Connection refused`, a time-out, `sslmode=require` against a server without TLS, a server certificate that doesn't verify |
| 4 | The server refuses the connection | a wrong password, no matching line in `pg_hba.conf`, a database that doesn't exist, too many connections |

When a benchmark client loses its connection during the run, it logs the error and reconnects, so
that doesn't end the run.
