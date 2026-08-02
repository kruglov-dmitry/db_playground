#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/explain-lookups.sh <postgres-url> <sample-file> [100|1000] [label] [table] [uuid|bigint]
# Writes results/<label>-<count>.plan.json and results/<label>-<count>.meta.json.
dsn=${1:?"PostgreSQL URL is required"}
sample=${2:?"CSV sample file is required"}
count=${3:-100}
label=${4:-"$(basename "$sample" .csv)"}
table=${5:-benchmark_items}
key_type=${6:-uuid}
results_dir=${RESULTS_DIR:-results}

case "$count" in 100|1000) ;; *) echo 'count must be 100 or 1000' >&2; exit 2;; esac
[[ "$table" =~ ^[a-z_][a-z0-9_]*$ ]] || { echo 'table must be a simple lowercase SQL identifier' >&2; exit 2; }
case "$key_type" in uuid|bigint) ;; *) echo 'key type must be uuid or bigint' >&2; exit 2;; esac
test "$(wc -l < "$sample" | tr -d ' ')" -ge "$count" || { echo "sample needs at least $count IDs" >&2; exit 2; }

ids="{$(head -n "$count" "$sample" | paste -sd, -)}"
mkdir -p "$results_dir"
plan_file="$results_dir/$label-$count.plan.json"
meta_file="$results_dir/$label-$count.meta.json"
plan_tmp=$(mktemp "$results_dir/.${label}-${count}.plan.XXXXXX")
meta_tmp=$(mktemp "$results_dir/.${label}-${count}.meta.XXXXXX")
trap 'rm -f "$plan_tmp" "$meta_tmp"' EXIT

psql "$dsn" --no-psqlrc --set=ON_ERROR_STOP=1 -q -v table="$table" <<'SQL'
ANALYZE :"table";
SQL

psql "$dsn" --no-psqlrc --set=ON_ERROR_STOP=1 --tuples-only --no-align -v ids="$ids" -v table="$table" -v key_type="$key_type" <<'SQL' >"$plan_tmp"
EXPLAIN (ANALYZE, BUFFERS, SETTINGS, FORMAT JSON)
SELECT id, inserted_at
FROM :"table"
WHERE id = ANY(:'ids':::key_type[]);
SQL

psql "$dsn" --no-psqlrc --set=ON_ERROR_STOP=1 --tuples-only --no-align \
  -v label="$label" -v lookup_count="$count" -v table="$table" -v key_type="$key_type" <<'SQL' >"$meta_tmp"
SELECT json_build_object(
  'label', :'label',
  'table', :'table',
  'key_type', :'key_type',
  'lookup_count', :lookup_count,
  'captured_at_utc', to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
  'server_version', version(),
  'server_version_num', current_setting('server_version_num')::integer,
  'database_size_bytes', pg_database_size(current_database()),
  'table_size_bytes', pg_relation_size(to_regclass(:'table')),
  'index_size_bytes', pg_indexes_size(to_regclass(:'table')),
  'row_count_estimate', (SELECT reltuples::bigint FROM pg_class WHERE oid = to_regclass(:'table'))
);
SQL

mv "$plan_tmp" "$plan_file"
mv "$meta_tmp" "$meta_file"
trap - EXIT
printf 'Saved plan: %s\nSaved metadata: %s\n' "$plan_file" "$meta_file"
