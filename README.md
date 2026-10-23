# pg_tps_optimizer

## TL/DR

### Binary

If you want to download the binary and run the tool directly:
```
curl -OL https://github.com/MannemSolutions/pg_tps_optimizer/releases/download/v0.1.3/pg_tps_optimizer_v0.1.3_x86_64-unknown-linux-musl.zip
unzip pg_tps_optimizer*.zip
./pg_tps_optimizer --dsn 'host=server1 user=postgres dbname=postgres password=password123' --max-wait 10s --min-samples 10 --range 200 --spread 10
```
**Note** that with the binary you need to set all the arguments as shown in the above example...

### Container

If you wanna use the container instead:
```
docker run -e PGHOST=server1,PGUSER=postgres,PGDATABASE=postgres,PGPASSWORD=password123 mannemsolutions/pg_tps_optimizer
```
**Note** that with the container you can set environment variables and leave other arguments default.
If you wanna change defaults, you do need to set all of them (might be fixed in future releases)...

### Arguments

If you wanna change options, you can:
- set `--max-wait` to set the timeout for a step
- set `--min-samples` to wait more timeslices of 200ms before accepting a step to be 'stable',
  printing results and moving to the next number of clients
- set `--range` to change the upper bound.
  The tool follows the fibonacci sequence, so (as an example) there is no difference between an upper bound of 99 and an upper bound of 100.
  In both cases 89 is the last step...
- set `--spread` to be more precise in when the tool decides a step is considered 'stable'.


Example:
```
docker run -e PGHOST=server1,PGUSER=postgres,PGDATABASE=postgres,PGPASSWORD=password123 mannemsolutions/pg_tps_optimizer \
  --max-wait 20s --min-samples 20 --range 200 --spread 5
```
This will do the following:
- wait 20 seconds before a step is timed out
- wait 20 samples (4 seconds) before checking if TPS and latency are stabilized
- not stop at 89, but at 144 clients (next step is 233, which is beyond 200)
- take TPS and latency for stable if they have a standard deviation of 5% max

### Example output
This could be the output of running the tool:
```
Initializing
dsn:dbname=postgres host=postgres sslcert=/host/config/tls/int_client/certs/postgres.pem sslkey=/host/config/tls/int_client/private/postgres.key.pem sslmode=prefer sslrootcert=/host/config/tls/int_server/certs/ca-chain-bundle.cert.pem user=postgres
transactional: false
prepared: false
min threads: 1 max threads: 101
max_wait: 20s, min_samples: 20, spread: 5
|---------------------|---------|-------------------------------------|-----------------------|
| Date       time     | Clients |              Performance            |       Postgres        |
|                     |         |-------------|---------|-------------|-----------|-----------|
|                     |         |    TPS      | Latency | TPS/Latency |   TPS     |    wal    |
|                     |         |             | (usec)  |             |           |    kB/s   |
|---------------------|---------|-------------|---------|-------------|-----------|-----------|
| 2023-05-18 06:40:20 |       1 |    9041.345 |   221.0 |      40.911 |  8592.240 |    511247 |
| 2023-05-18 06:40:23 |       2 |   12336.280 |   324.0 |      38.075 | 10439.705 |        -1 |
| 2023-05-18 06:40:25 |       3 |   15959.637 |   376.0 |      42.446 | 16725.326 |        -1 |
| 2023-05-18 06:40:27 |       5 |   19727.549 |   516.0 |      38.232 | 17737.223 |    403844 |
| 2023-05-18 06:40:30 |       8 |   21945.422 |   726.0 |      30.228 | 20931.607 |    421418 |
| 2023-05-18 06:40:32 |      13 |   23098.930 |  1125.0 |      20.532 | 22236.824 |    198932 |
| 2023-05-18 06:40:35 |      21 |   20485.059 |  2070.0 |       9.896 | 21066.568 |      2138 |
| 2023-05-18 06:40:38 |      34 |   19123.058 |  3703.0 |       5.164 | 16604.666 |        -1 |
| 2023-05-18 06:40:42 |      55 |   19289.153 |  6025.0 |       3.202 | 18482.289 |     88422 |
| 2023-05-18 06:40:56 |      89 |   12257.733 | 12854.0 |       0.953 | 12144.653 |    470456 |
| 2023-05-18 06:41:13 |     133 |    8232.331 | 20325.0 |       0.405 |  8234.765 |    278341 |
|---------------------|---------|-------------|---------|-------------|-----------|-----------|
Stopping, but lets give the threads some time to stop
Finished
```

This is run on a Macbook M1 ;)
### Environment variables
pg_tps_optimizer supports these environment variables to be used.
See [docs/connecting.md](docs/connecting.md) for how connection settings, TLS (`sslmode`) and certificates work.

PGHOST=/tmp
PGUSER=$(id -u -n) # Defaults to current user
PGDATABASE=${PGUSER}
PGPASSWORD=***** # Defaults to emptystring which basically cannot work. For ident, trust, cert, etc. Just set a dummy password.
PGSSLMODE=prefer
PGSSLCERT=~/.postgresql/postgresql.crt
PGSSLKEY=~/.postgresql/postgresql.key
PGSSLROOTCERT=~/.postgresql/root.crt

PGTPSSOURCE="" # The source actually is combined with the values from the PG... ENV variables.
PGTPSQUERY="select * from pg_tables"
PGTPSPREPARED=false
PGTPSTRANSACTIONAL-false
PGTPSRANGE=1:1000
PGTPSMAXWAIT=10s
PGTPSSPREAD=10.0
PGTPSMINSAMPLES=10

**Note** that Argumnets have precedence over Environment variables.

Example:
```
docker run -e PGHOST=server1,PGUSER=postgres,PGDATABASE=postgres,PGPASSWORD=password123,\
PGTPSMAXWAIT=20s,PGTPSMINSAMPLES=20,PGTPSRANGE=200,PGTPSSPREAD=5 mannemsolutions/pg_tps_optimizer
```
This will do the same as the previous example.

## General information
This repo is about doing some load testing on Postgres.
The concept is that depending on #clients first tps increases, and then latency increases, as such tps/latency works like a curve.
We want to find the optimal #clients with max utilization (max tps against lowest latency).

Max utilization is system dependant:
* system: number of cpu, memory, storage performance, etc.
* max_connections (but only little impact)
* table, indexes, etc. (which is why we allow to set query and define own strcture and dataset)
* postgres config (which could be tuned by run pg_cpu_load_tester to multiple configs and recording #clients, #tps and #latency)

How it works:
* We run workload with one client and check tps, latency
* we wait until they stabilize and then we print #clients, #tps, latency(ms) and tps/latency
* We increase the number of clients according to fibonacci (2, 3, 5, 8, etc) and repeat the process

In the end you have a report (table) describing TPS and Latency depending on the number of clients.

## How can you leverage this

Depending on your needs you can:
- deduct the number of clients that can do the number of TPS as required
- deduct the number of clients that still has a limited impact on latency (and with that also impact on the rest of the infrastructure)
- deduct the optimal middel ground of the two
- find an optimal starting point for running pg_bench
- compare these stats across multiple arcchitectures
- track these stats as you are applying changes to your setup

## Why another tool next to pg_becnch and other existing tools

Most tools are typically designed to be used as a benchmark investigation.
Properly benchmarking is more of an academic approach and requires a lot of preparation, human effort, hardware resources, etc.
And there are many situations where the investment is too big and the results are not entirely what you are looking for.
Some examples are:
- you wanna run a benchmark, but don;t know the optimal number of clients
- you wanna tune the number of connections in the connection pool for optimal performance
- you wanna know if increasing the number of clients could be benficcial to fixing a performance issue without impacting the production workload
  and as such you wanna know the performance responce of the architecture
- you wanna track performance as you tune configuration, apply environmental changes, etc. and don't want to invest the effort in doing constant full benchmarks
- you want a more broad perspective on the responsiveness of the architecture (instead of, or in addition to) a specific benchmark

In all of these cases pg_tps_optimizer can help you to gather the information.
And of coarse, you can always check by running one pgbench run, just to see if they agree.

## License
We love ur software and want them to grow. Therefore we embrace open Source, and invite you to join the community and contribute.
Therefore we feel that GPL-3.0 license best meets the needs of our users and the community.
In general:
- feel free to use, distribute and even change the code
- if you wanna distribute changed versions we wuld appreciate if yu also upstream your changes so we can expand this project to be even more awesome
Thank you...

## Contributing
We are open source, and are always open to contributions.
- If you experience issues, please submit a github [issue](https://github.com/MannemSolutions/pg_tps_optimizer/issues).
- If you wanna expand features, or fix bugs, please submit a github [Pull Request](https://github.com/MannemSolutions/pg_tps_optimizer/pulls).
