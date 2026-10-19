-- Durable result inbox: admitted heartbeat batches wait here until a DuckDB
-- transfer batch containing them commits. AUTOINCREMENT keeps sequences and
-- transfer batch IDs from ever being reused after retirement deletes the rows.

CREATE TABLE batches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    first_seq INTEGER NOT NULL,
    last_seq INTEGER NOT NULL,
    records INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    assigned_at INTEGER NOT NULL
);

-- One row per admission: an agent batch, a group of built-in results, or evaluator entries.
CREATE TABLE inbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    received_at INTEGER NOT NULL,
    format INTEGER NOT NULL,
    -- heartbeats plus monitor-level entries
    entries INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    payload BLOB NOT NULL,
    batch_id INTEGER REFERENCES batches (id)
);

CREATE INDEX inbox_pending ON inbox (seq) WHERE batch_id IS NULL;
CREATE INDEX inbox_batch ON inbox (batch_id) WHERE batch_id IS NOT NULL;

-- Agent batch IDs seen per probe. A retry with the same content gets the original
-- sequence back; a retry with other content is refused. Rows outlive retirement
-- until pruned, so an agent retrying after transfer still finds its batch.
CREATE TABLE agent_batches (
    probe_id INTEGER NOT NULL,
    batch_key TEXT NOT NULL,
    fingerprint BLOB NOT NULL,
    seq INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (probe_id, batch_key)
) WITHOUT ROWID;

CREATE INDEX agent_batches_received ON agent_batches (received_at);
