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

## Compare lookup plans

The loader saves real inserted IDs, avoiding the misleading “all misses” plan that random lookup values would produce.

```bash
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-v4.csv 100
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54317/benchmark samples/pg17-v4.csv 1000
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-v7.csv 100
scripts/explain-lookups.sh postgres://benchmark:benchmark@localhost:54318/benchmark samples/pg18-v7.csv 1000
```

Every run creates two files under `results/`: a `*.plan.json` execution plan and a `*.meta.json` snapshot containing database/index sizes, server version, lookup count, and timestamp. These are the inputs for the comparison visualizer; they are intentionally excluded from Git. Optionally add a fourth argument to use a custom label.

## Visualize the comparison

After all four lookup runs are complete, generate a self-contained report:

```bash
python3 scripts/visualize-results.py
```

Open `results/comparison.html`. It charts execution time and buffer reads for 100 and 1,000 lookups, and includes the full plan and storage comparison tables. Use `--input` and `--output` to customize file locations.

To compare on-disk footprint after a load:

```bash
psql postgres://benchmark:benchmark@localhost:54317/benchmark -c "SELECT pg_size_pretty(pg_relation_size('benchmark_items')), pg_size_pretty(pg_indexes_size('benchmark_items'));"
psql postgres://benchmark:benchmark@localhost:54318/benchmark -c "SELECT pg_size_pretty(pg_relation_size('benchmark_items')), pg_size_pretty(pg_indexes_size('benchmark_items'));"
```

For a smoke test, use `--rows 1000000` first. The loader prints throughput every five seconds and exits only after PostgreSQL has accepted the COPY stream.

17:
kruglovdmitry@MacBookPro pg_uuid_test % psql postgres://benchmark:benchmark@localhost:54317/benchmark -c "SELECT pg_size_pretty(pg_relation_size('benchmark_items')), pg_size_pretty(pg_indexes_size('benchmark_items'));"
 pg_size_pretty | pg_size_pretty 
----------------+----------------
 9953 MB        | 7920 MB
(1 row)

kruglovdmitry@MacBookPro pg_uuid_test % psql postgres://benchmark:benchmark@localhost:54318/benchmark -c "SELECT pg_size_pretty(pg_relation_size('benchmark_items')), pg_size_pretty(pg_indexes_size('benchmark_items'));"
 pg_size_pretty | pg_size_pretty 
----------------+----------------
 0 bytes        | 8192 bytes
(1 row)
