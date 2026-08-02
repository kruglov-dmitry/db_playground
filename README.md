# PostgreSQL UUIDv4 vs UUIDv7 benchmark

This project compares PostgreSQL 17 using random UUIDv4 primary keys with PostgreSQL 18 using time-ordered UUIDv7 primary keys. Each service starts with an empty database and the same two-column table. Their defaults are `gen_random_uuid()` (v4) and PostgreSQL 18's native `uuidv7()` respectively; the loader explicitly supplies values so both runs use the same high-throughput path. The Rust loader uses PostgreSQL's binary `COPY` protocol, which is a much better fit than row-by-row Python inserts for a 200M-row test.

## Start fresh databases

```bash
docker compose down -v
docker compose up -d --wait
cargo build --release
mkdir -p samples
```

`down -v` deliberately discards prior benchmark data. Omit it when continuing an existing run.

## Load 200 million rows

Run these one at a time if both databases share a disk; that keeps I/O contention out of the comparison.

```bash
target/release/pg-uuid-loader --dsn postgres://benchmark:benchmark@localhost:54317/benchmark --uuid v4 --rows 200000000 --truncate --sample-output samples/pg17-v4.csv
target/release/pg-uuid-loader --dsn postgres://benchmark:benchmark@localhost:54318/benchmark --uuid v7 --rows 200000000 --truncate --sample-output samples/pg18-v7.csv
```

The PG18 table has a check constraint that rejects non-v7 UUIDs. UUIDv7 values are generated according to RFC 9562 in the loader, so the benchmark does not depend on a particular server-side UUID extension.

## Test both UUID types on both PostgreSQL versions

Fresh containers create `benchmark_items_v4` and `benchmark_items_v7` automatically. For databases that already exist, add the variant tables once:

```bash
psql postgres://benchmark:benchmark@localhost:54317/benchmark -f sql/variant-tables.sql
psql postgres://benchmark:benchmark@localhost:54318/benchmark -f sql/variant-tables.sql
```

Then load the two additional combinations. These use separate tables, so the existing runs stay intact:

```bash
target/release/pg-uuid-loader --dsn postgres://benchmark:benchmark@localhost:54318/benchmark --uuid v4 --table benchmark_items_v4 --rows 200000000 --truncate --sample-output samples/pg18-v4.csv
target/release/pg-uuid-loader --dsn postgres://benchmark:benchmark@localhost:54317/benchmark --uuid v7 --table benchmark_items_v7 --rows 200000000 --truncate --sample-output samples/pg17-v7.csv
```

## BIGINT primary-key baseline

The project also includes the same sequential `BIGINT PRIMARY KEY` test on both PostgreSQL versions. It measures an append-only integer-key baseline; unlike UUIDv4, these values are not random.

```bash
target/release/pg-uuid-loader --dsn postgres://benchmark:benchmark@localhost:54317/benchmark --key-type bigint --table benchmark_items_bigint --rows 200000000 --truncate --sample-output samples/pg17-bigint.csv
target/release/pg-uuid-loader --dsn postgres://benchmark:benchmark@localhost:54318/benchmark --key-type bigint --table benchmark_items_bigint --rows 200000000 --truncate --sample-output samples/pg18-bigint.csv
```

## Compare lookup plans

The loader saves real inserted IDs, avoiding the misleading “all misses” plan that random lookup values would produce.

```bash
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-v4.csv 100
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-v4.csv 1000
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-v7.csv 100
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-v7.csv 1000
```

For a variant table, pass the optional label and table name:

```bash
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-v4.csv 100 pg18-v4 benchmark_items_v4
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-v4.csv 1000 pg18-v4 benchmark_items_v4
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-v7.csv 100 pg17-v7 benchmark_items_v7
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-v7.csv 1000 pg17-v7 benchmark_items_v7
```

For the BIGINT tables, use the final `bigint` argument:

```bash
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-bigint.csv 100 pg17-bigint benchmark_items_bigint bigint
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-bigint.csv 1000 pg17-bigint benchmark_items_bigint bigint
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-bigint.csv 100 pg18-bigint benchmark_items_bigint bigint
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-bigint.csv 1000 pg18-bigint benchmark_items_bigint bigint
```

Every run creates two files under `results/`: a `*.plan.json` execution plan and a `*.meta.json` snapshot containing database/index sizes, server version, lookup count, and timestamp. These are the inputs for the comparison visualizer; they are intentionally excluded from Git. Optionally add a fourth argument to use a custom label.

## Visualize the comparison

After the eight lookup runs are complete, generate the report:

```bash
python3 scripts/visualize-results.py
```

Open `results/comparison.html`. It automatically includes every complete plan/metadata pair in `results/`, including UUID and BIGINT combinations. After each successful load, the loader saves its duration and throughput as `results/<label>.load.json`; the report reads those files automatically. The label defaults to the sample filename (for example, `samples/pg18-v4.csv` becomes `pg18-v4`).

To compare on-disk footprint after a load:

```bash
psql postgres://benchmark:benchmark@localhost:54317/benchmark -c "SELECT pg_size_pretty(pg_relation_size('benchmark_items')), pg_size_pretty(pg_indexes_size('benchmark_items'));"
psql postgres://benchmark:benchmark@localhost:54318/benchmark -c "SELECT pg_size_pretty(pg_relation_size('benchmark_items')), pg_size_pretty(pg_indexes_size('benchmark_items'));"
```

For a smoke test, use `--rows 1000000` first. The loader prints throughput every five seconds and exits only after PostgreSQL has accepted the COPY stream.
