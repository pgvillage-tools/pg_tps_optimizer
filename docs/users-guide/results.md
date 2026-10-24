# Reading the results

A run prints its settings, and then one line per step (number of clients):

```
Initializing
dsn:dbname='postgres' host='db1.example.com' password='*****' port='5432' sslcert='' sslkey='' sslmode='prefer' sslrootcert='' user='postgres'
transactional: false
prepared: false
min threads: 1 max threads: 200
max_wait: 10s, min_samples: 10, spread: 10
|---------------------|---------|-----------------------------------------|-----------------------|
| Date       time     | Clients |                 Performance             |       Postgres        |
|                     |         |---------------|-----------|-------------|-----------|-----------|
|                     |         |      TPS      |  Latency  | TPS/Latency |   TPS     |    wal    |
|                     |         |               |   (usec)  |             |           |    kB/s   |
|---------------------|---------|---------------|-----------|-------------|-----------|-----------|
| 2026-10-24 07:49:41 |       1 |      4013.761 |     498.0 |       8.060 |  3655.436 |     263.2 |
| 2026-10-24 07:49:44 |       2 |      7620.030 |     525.0 |      14.514 |  6577.299 |     428.4 |
| 2026-10-24 07:49:47 |       3 |      9110.991 |     658.0 |      13.846 |  8853.864 |     510.2 |
| 2026-10-24 07:49:49 |       5 |     11084.831 |     910.0 |      12.181 | 10330.394 |     616.6 |
| 2026-10-24 07:49:52 |       8 |     13298.201 |    1167.0 |      11.395 | 13309.817 |     756.6 |
| 2026-10-24 07:49:54 |      13 |     16196.843 |    1505.0 |      10.762 | 18080.017 |     943.4 |
| 2026-10-24 07:49:59 |      21 |     20171.060 |    2068.0 |       9.754 | 18787.618 |    1138.1 |
| 2026-10-24 07:50:03 |      34 |     21561.297 |    3171.0 |       6.800 | 21995.177 |    1207.4 |
| 2026-10-24 07:50:07 |      55 |     22192.413 |    4962.0 |       4.472 | 22271.669 |    1321.8 |
| 2026-10-24 07:50:12 |      89 |     23003.485 |    7679.0 |       2.996 | 22440.198 |    1320.6 |
| 2026-10-24 07:50:23 |     144 | *   23712.167 |   11790.0 |       2.011 | 24739.325 |    1374.3 |
|---------------------|---------|---------------|-----------|-------------|-----------|-----------|
* Samples marked with '*' did not stabilize before max-wait.
Stopping, but lets give the threads some time to stop
Finished
```

The password is never printed.

## Columns

| Column | Meaning |
|---|---|
| Date time | When the step finished |
| Clients | The number of clients (connections) running the workload in this step |
| Performance: TPS | Transactions per second, as measured by the clients |
| Performance: Latency (usec) | The average time a transaction took, in microseconds |
| Performance: TPS/Latency | TPS divided by latency: a measure for how efficiently the clients are served |
| Postgres: TPS | Transactions per second according to PostgreSQL (`pg_stat_database`), over the whole step (also while waiting for it to become stable): committed and rolled back transactions in all databases, so including other activity on the server. PostgreSQL updates these statistics about once per second, so short steps can differ more from the clients' TPS |
| Postgres: wal kB/s | The amount of WAL (write-ahead log) PostgreSQL wrote per second during the step, in kB, for the whole server |

A `*` before the TPS means the step didn't become stable within `--max-wait`: the numbers are the
mean of the last samples, but they varied more than `--spread`. More clients, or a busy server,
usually need more time; try a larger `--max-wait`.

## Interpreting the results

In the example above (PostgreSQL 18 in a small virtual machine):

- **TPS** grows quickly up to 21 to 34 clients (about 21,500 TPS), and hardly grows after that:
  the server is saturated.
- **Latency** keeps growing with every step: from 0.5 ms with one client to almost 12 ms with 144
  clients. More clients means more waiting.
- **TPS/Latency** is highest at 2 to 3 clients: transactions are fast, but the server isn't used
  fully yet.
- **Postgres TPS** follows the TPS of the clients, so there was no other noticeable load on the
  server. **wal** grows with the TPS: about 60 to 70 bytes of WAL per transaction.

So, depending on what you need:

- for (nearly) the most TPS, use around 34 clients;
- for the lowest latency with reasonable TPS, use 3 to 8 clients;
- more than 55 clients mostly adds latency. That's a good upper limit for a connection pool in
  front of this server.

The numbers depend on the hardware, the PostgreSQL configuration and the workload. pg_tps_optimizer
runs a very small update (see [the workload](usage.md#the-workload)), so it shows the limits of the
server and the connection handling, not of your own queries.
