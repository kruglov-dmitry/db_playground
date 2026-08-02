-- Apply to either PostgreSQL 17 or 18 to add tables for the opposite UUID type.
-- The loader explicitly provides IDs, so these tables have no server-side UUID default.
CREATE TABLE IF NOT EXISTS benchmark_items_v4 (
    id uuid PRIMARY KEY,
    inserted_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE IF NOT EXISTS benchmark_items_v7 (
    id uuid PRIMARY KEY,
    inserted_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

-- Sequential BIGINT is the append-only primary-key baseline for both versions.
CREATE TABLE IF NOT EXISTS benchmark_items_bigint (
    id bigint PRIMARY KEY,
    inserted_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
