#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/explain-lookups.sh <postgres-url> <sample-file> [100|1000] [label]
# Writes results/<label>-<count>.plan.json and results/<label>-<count>.meta.json.
dsn=${1:?"PostgreSQL URL is required"}
sample=${2:?"CSV sample file is required"}
count=${3:-100}
label=${4:-"$(basename "$sample" .csv)"}
results_dir=${RESULTS_DIR:-results}

case "$count" in 100|1000) ;; *) echo 'count must be 100 or 1000' >&2; exit 2;; esac
test "$(wc -l < "$sample" | tr -d ' ')" -ge "$count" || { echo "sample needs at least $count IDs" >&2; exit 2; }

ids="{$(head -n "$count" "$sample" | paste -sd, -)}"
mkdir -p "$results_dir"
plan_file="$results_dir/$label-$count.plan.json"
meta_file="$results_dir/$label-$count.meta.json"
plan_tmp=$(mktemp "$results_dir/.${label}-${count}.plan.XXXXXX")
meta_tmp=$(mktemp "$results_dir/.${label}-${count}.meta.XXXXXX")
trap 'rm -f "$plan_tmp" "$meta_tmp"' EXIT

psql "$dsn" --no-psqlrc --set=ON_ERROR_STOP=1 -q -c 'ANALYZE benchmark_items'

psql "$dsn" --no-psqlrc --set=ON_ERROR_STOP=1 --tuples-only --no-align -v ids="$ids" <<'SQL' >"$plan_tmp"
EXPLAIN (ANALYZE, BUFFERS, SETTINGS, FORMAT JSON)
SELECT id, inserted_at
FROM benchmark_items
WHERE id = ANY(:'ids'::uuid[]);
SQL

psql "$dsn" --no-psqlrc --set=ON_ERROR_STOP=1 --tuples-only --no-align \
  -v label="$label" -v lookup_count="$count" <<'SQL' >"$meta_tmp"
SELECT json_build_object(
  'label', :'label',
  'lookup_count', :lookup_count,
  'captured_at_utc', to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
  'server_version', version(),
  'server_version_num', current_setting('server_version_num')::integer,
  'database_size_bytes', pg_database_size(current_database()),
  'table_size_bytes', pg_relation_size('benchmark_items'),
  'index_size_bytes', pg_indexes_size('benchmark_items'),
  'row_count_estimate', (SELECT reltuples::bigint FROM pg_class WHERE oid = 'benchmark_items'::regclass)
);
SQL

mv "$plan_tmp" "$plan_file"
mv "$meta_tmp" "$meta_file"
trap - EXIT
printf 'Saved plan: %s\nSaved metadata: %s\n' "$plan_file" "$meta_file"
