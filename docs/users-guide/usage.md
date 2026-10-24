# Usage

```
pg_tps_optimizer [OPTIONS]
```

Without options, pg_tps_optimizer connects with the default
[connection settings](connecting.md) and runs with 1 up to 987 clients.

## Options

Every option can also be set with an environment variable. An option on the command line wins over
the environment variable, which wins over the default.

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `-d`, `--dsn` | `PGTPSSOURCE` | | Connection string; see [Connecting to PostgreSQL](connecting.md) |
| `-r`, `--range` | `PGTPSRANGE` | `1:1000` | The number of clients to run with: `min:max`, or only `max` |
| `-m`, `--min-samples` | `PGTPSMINSAMPLES` | `10` | The number of samples (of 200ms) to check if a step is stable |
| `-s`, `--spread` | `PGTPSSPREAD` | `10` | The maximum spread (in %) of TPS and latency for a stable step |
| `-M`, `--max-wait` | `PGTPSMAXWAIT` | `10s` | How long to wait for a step to become stable, like `10s`, `1m30s` or `20` (seconds) |
| `-t`, `--transactional` | `PGTPSTRANSACTIONAL` | off | Run every query in its own transaction (`begin` / `commit`) |
| `-p`, `--prepared` | `PGTPSPREPARED` | off | Run the query as a prepared statement |
| `-q`, `--query` | `PGTPSQUERY` | | Not used yet, see below |
| `-o`, `--output` | `PGTPSOUTPUT` | `table` | `table`: a table while running; `json`: a JSON document at the end, see [JSON output](results.md#json-output) |
| `-h`, `--help` | | | Show the options |
| `-V`, `--version` | | | Show the version |

For the on/off options, the environment variable can be `true`/`false`, `1`/`0`, `yes`/`no` or
`on`/`off`.

!!! note
    `--query` is accepted, but not used yet: the clients always run the built-in workload (see
    below). Making the query configurable is
    [issue #34](https://github.com/pgvillage-tools/pg_tps_optimizer/issues/34).

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

pg_tps_optimizer creates a table `pg_tps_optimizer (id oid)` in the database it connects to, if it
doesn't exist yet, and empties it. Every client inserts a row with its own id, and then runs this
statement as fast as it can:

```sql
update pg_tps_optimizer set id = $1 where id = $1
```

So the user needs permission to create a table (or the table must exist), and to insert and update
it. The table is left behind after the run.

With `--transactional`, every update runs in its own transaction (`begin`, update, `commit`). With
`--prepared`, every client prepares the update once (again after a reconnect), and then only
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

| Exit code | Meaning |
|---|---|
| 0 | The run finished |
| 1 | An error, like a connection error or an invalid `--max-wait`; the message is printed to stderr |
| 2 | The options or `PGTPS*` environment variables can't be parsed, like `--spread abc` |

Separate exit codes per kind of error are planned in
[issue #57](https://github.com/pgvillage-tools/pg_tps_optimizer/issues/57).
