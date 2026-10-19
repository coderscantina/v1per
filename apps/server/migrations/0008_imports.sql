-- Kuma imports (milestone 10, decision 0007): one run per target node, its step checkpoints,
-- the Kuma rows behind the records it created, and the inventory of what it did not map.

-- One import into one node, ever: a second one into the same node is refused. `running` means
-- in progress or interrupted; `serve` refuses to start until it is `complete`. `fingerprint`
-- is the SHA-256 of the source file (and its WAL), so a resume against a changed source is
-- refused. `options` are the run's settings and `clock_ms` its pinned start, which the raw
-- heartbeat window is measured from. `manifest` is the reconciliation JSON.
CREATE TABLE import_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id     INTEGER NOT NULL UNIQUE REFERENCES nodes (id) ON DELETE CASCADE,
    source      TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    options     TEXT NOT NULL,
    clock_ms    INTEGER NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('running', 'complete')),
    started_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    finished_at INTEGER,
    manifest    TEXT
) STRICT;

-- Checkpoint of each step: `control`, `stats`, `heartbeats`, `reconcile`. `cursor` is the last
-- source position a step committed and `chunks` how many chunks it committed. Heartbeat chunk
-- `n` enters the inbox under the batch key `kuma-import:<run>:<n>`, so a chunk repeated after
-- a crash is recognized there and stored once.
CREATE TABLE import_steps (
    run_id     INTEGER NOT NULL REFERENCES import_runs (id) ON DELETE CASCADE,
    step       TEXT NOT NULL CHECK (step IN ('control', 'stats', 'heartbeats', 'reconcile')),
    cursor     TEXT,
    chunks     INTEGER NOT NULL DEFAULT 0,
    rows       INTEGER NOT NULL DEFAULT 0,
    skipped    INTEGER NOT NULL DEFAULT 0,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, step)
) STRICT, WITHOUT ROWID;

-- The v1per record each Kuma row became: `kind` is `node`, `monitor`, `channel`,
-- `maintenance`, `page`, `incident` or `user`; `target_id` the v1per ID as text. History steps
-- map Kuma monitor IDs through it.
CREATE TABLE import_objects (
    run_id    INTEGER NOT NULL REFERENCES import_runs (id) ON DELETE CASCADE,
    kind      TEXT NOT NULL,
    source_id INTEGER NOT NULL,
    target_id TEXT NOT NULL,
    PRIMARY KEY (run_id, kind, source_id)
) STRICT, WITHOUT ROWID;

-- What was not mapped, or mapped with a loss, with the reason. `columns` lists the source
-- columns kept; their values are sealed in `sealed` under `import-inventory:<id>`, since Kuma
-- keeps credentials in plain text (decision 0006).
CREATE TABLE import_inventory (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id       INTEGER NOT NULL REFERENCES import_runs (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    label        TEXT NOT NULL,
    reason       TEXT NOT NULL,
    source_table TEXT NOT NULL,
    source_id    INTEGER NOT NULL,
    columns      TEXT NOT NULL,
    sealed       BLOB NOT NULL
) STRICT;

CREATE INDEX import_inventory_run ON import_inventory (run_id, id);
