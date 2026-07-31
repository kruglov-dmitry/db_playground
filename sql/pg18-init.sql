CREATE TABLE benchmark_items (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    inserted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT benchmark_items_uuidv7 CHECK ((get_byte(uuid_send(id), 6) >> 4) = 7)
);
