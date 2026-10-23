# pg_tps_optimizer

pg_tps_optimizer finds the number of clients that gives PostgreSQL the most transactions per second
(TPS), at the lowest latency.

It runs a workload with an increasing number of clients (1, 2, 3, 5, 8, 13, ...), waits until TPS
and latency are stable for each step, and reports them, so you can see how your setup responds to
more clients:

```
| Date       time     | Clients |       TPS     |  Latency  | TPS/Latency |
|                     |         |               |   (usec)  |             |
|---------------------|---------|---------------|-----------|-------------|
| 2023-05-18 06:40:20 |       1 |      9041.345 |     221.0 |      40.911 |
| 2023-05-18 06:40:27 |       5 |     19727.549 |     516.0 |      38.232 |
| 2023-05-18 06:40:32 |      13 |     23098.930 |    1125.0 |      20.532 |
| 2023-05-18 06:40:42 |      55 |     19289.153 |    6025.0 |       3.202 |
```

## Quick start

Download the binary for your platform from the
[releases](https://github.com/pgvillage-tools/pg_tps_optimizer/releases), and run:

```
pg_tps_optimizer --dsn 'host=db1.example.com user=postgres dbname=postgres' --range 200
```

or use the container image:

```
docker run --rm -e PGHOST=db1.example.com -e PGUSER=postgres -e PGPASSWORD=secret \
  mannemsolutions/pg_tps_optimizer --range 200
```

**Note:** pg_tps_optimizer creates a small table (`pg_tps_optimizer`) in the database it connects
to, and generates as much load as the server can handle.

## Documentation

The documentation is at **https://pgvillage-tools.github.io/pg_tps_optimizer/**
(sources in [`docs/`](docs/)):

- [Installation](docs/users-guide/installation.md)
- [Usage](docs/users-guide/usage.md): options, environment variables and how it works
- [Reading the results](docs/users-guide/results.md)
- [Connecting to PostgreSQL](docs/users-guide/connecting.md): connection settings, TLS and service
  files
- [Developer guide](docs/developers-guide/building.md): building, testing and releasing

## License

pg_tps_optimizer is open source, under the [GNU General Public License v3.0](LICENSE).

## Contributing

We are always open to contributions:

- Found a problem or have an idea? Open an
  [issue](https://github.com/pgvillage-tools/pg_tps_optimizer/issues).
- Want to fix or add something? Open a
  [pull request](https://github.com/pgvillage-tools/pg_tps_optimizer/pulls); see the
  [developer guide](docs/developers-guide/building.md).
