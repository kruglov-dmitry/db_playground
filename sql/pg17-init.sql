CREATE TABLE benchmark_items (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
