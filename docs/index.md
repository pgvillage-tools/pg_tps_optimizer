# pg_tps_optimizer

pg_tps_optimizer finds the number of clients that gives PostgreSQL the most transactions per second
(TPS), at the lowest latency.

It starts a workload with one client, waits until TPS and latency are stable, and then adds clients
following the Fibonacci sequence (1, 2, 3, 5, 8, 13, ...). For every step it reports TPS and
latency, so you can see how your setup responds to more clients:

```
| Date       time     | Clients |       TPS     |  Latency  | TPS/Latency |
|                     |         |               |   (usec)  |             |
|---------------------|---------|---------------|-----------|-------------|
| 2023-05-18 06:40:20 |       1 |      9041.345 |     221.0 |      40.911 |
| 2023-05-18 06:40:27 |       5 |     19727.549 |     516.0 |      38.232 |
| 2023-05-18 06:40:32 |      13 |     23098.930 |    1125.0 |      20.532 |
| 2023-05-18 06:40:42 |      55 |     19289.153 |    6025.0 |       3.202 |
```

At first, TPS goes up with more clients. At some point, more clients only add latency, and
eventually TPS drops as well. pg_tps_optimizer shows you where that happens.

## Quick start

```
pg_tps_optimizer --dsn 'host=db1.example.com user=postgres dbname=postgres' --range 200
```

or with the container image:

```
docker run --rm -e PGHOST=db1.example.com -e PGUSER=postgres -e PGPASSWORD=secret \
  mannemsolutions/pg_tps_optimizer --range 200
```

!!! warning
    By default, pg_tps_optimizer creates a small table (`pg_tps_optimizer`) in the database it
    connects to, which it drops after the run, and it generates as much load as the server can
    handle. Don't point it at a production database unless you know what you are doing.

See [Installation](users-guide/installation.md) and [Usage](users-guide/usage.md) for more.

## What you can use it for

- Find the number of clients that gives the TPS you need
- Find the number of clients that still has little impact on latency (and with that, on the rest
  of your infrastructure), or the middle ground between the two
- Tune the size of a connection pool
- Find a good starting point for a pgbench run
- Compare architectures, or follow the effect of configuration changes over time

## Why another tool next to pgbench?

Proper benchmarking is an academic exercise: it takes a lot of preparation, effort and hardware,
and the results are not always what you are looking for. Often you just want to know how your
setup responds to more clients, for example:

- you want to run a benchmark, but don't know the best number of clients
- you want to tune the number of connections in a connection pool
- you want to know if more clients would help with a performance issue
- you want to follow performance while you tune your configuration, without running full benchmarks
  every time

In those cases, pg_tps_optimizer gives you the information quickly. You can always check the
result with a single pgbench run.
