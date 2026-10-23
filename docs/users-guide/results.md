# Reading the results

A run prints its settings, and then one line per step (number of clients):

```
Initializing
dsn:dbname='postgres' host='db1.example.com' password='*****' port='5432' sslcert='' sslkey='' sslmode='prefer' sslrootcert='' user='postgres'
transactional: false
prepared: false
min threads: 1 max threads: 200
max_wait: 20s, min_samples: 20, spread: 5
|---------------------|---------|-----------------------------------------|-----------------------|
| Date       time     | Clients |                 Performance             |       Postgres        |
|                     |         |---------------|-----------|-------------|-----------|-----------|
|                     |         |      TPS      |  Latency  | TPS/Latency |   TPS     |    wal    |
|                     |         |               |   (usec)  |             |           |    kB/s   |
|---------------------|---------|---------------|-----------|-------------|-----------|-----------|
| 2023-05-18 06:40:20 |       1 |      9041.345 |     221.0 |      40.911 |  8592.240 |    511247 |
| 2023-05-18 06:40:23 |       2 |     12336.280 |     324.0 |      38.075 | 10439.705 |        -1 |
| 2023-05-18 06:40:25 |       3 |     15959.637 |     376.0 |      42.446 | 16725.326 |        -1 |
| 2023-05-18 06:40:27 |       5 |     19727.549 |     516.0 |      38.232 | 17737.223 |    403844 |
| 2023-05-18 06:40:30 |       8 |     21945.422 |     726.0 |      30.228 | 20931.607 |    421418 |
| 2023-05-18 06:40:32 |      13 |     23098.930 |    1125.0 |      20.532 | 22236.824 |    198932 |
| 2023-05-18 06:40:35 |      21 |     20485.059 |    2070.0 |       9.896 | 21066.568 |      2138 |
| 2023-05-18 06:40:38 |      34 |     19123.058 |    3703.0 |       5.164 | 16604.666 |        -1 |
| 2023-05-18 06:40:42 |      55 |     19289.153 |    6025.0 |       3.202 | 18482.289 |     88422 |
| 2023-05-18 06:40:56 |      89 |   * 12257.733 |   12854.0 |       0.953 | 12144.653 |    470456 |
| 2023-05-18 06:41:13 |     144 |   *  8232.331 |   20325.0 |       0.405 |  8234.765 |    278341 |
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
| Postgres: TPS | Transactions per second according to PostgreSQL (`pg_stat_database`): committed and rolled back transactions in all databases, so including other activity on the server |
| Postgres: wal | Meant to show the amount of WAL written per second, see the warning below |

A `*` before the TPS means the step didn't become stable within `--max-wait`: the numbers are the
mean of the last samples, but they varied more than `--spread`. More clients, or a busy server,
usually need more time; try a larger `--max-wait`.

!!! warning "The wal column is not reliable"
    The WAL per second is currently calculated from overlapping periods, which can make it negative
    (shown as `-1`) or too high. The value is also in bytes per second, not kB/s.

## Interpreting the results

In the example above:

- **TPS** grows up to 13 clients (23,099 TPS), and drops with more clients: the server is saturated.
- **Latency** keeps growing with every step: more clients means more waiting.
- **TPS/Latency** is highest at 1 to 5 clients: transactions are fast, but the server isn't used
  fully yet.

So, depending on what you need:

- for the most TPS, use around 13 clients;
- for the lowest latency with reasonable TPS, use 5 to 8 clients;
- more than 21 clients only adds latency, and lowers TPS. That's a good upper limit for a
  connection pool in front of this server.

The numbers depend on the hardware, the PostgreSQL configuration and the workload. pg_tps_optimizer
runs a very small update (see [the workload](usage.md#the-workload)), so it shows the limits of the
server and the connection handling, not of your own queries.
