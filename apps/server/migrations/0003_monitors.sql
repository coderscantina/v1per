-- Monitors, probes and evaluated state (milestone 05, monitoring contract). Heartbeats and
-- rollups live in DuckDB; this file holds what decides status and what alerts read.

-- Probes that run checks. The built-in probe exists from the start; remote agents follow in
-- milestone 06. IDs are never reused, so heartbeats keep pointing at the probe that ran them.
CREATE TABLE probes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id    INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    builtin    INTEGER NOT NULL DEFAULT 0 CHECK (builtin IN (0, 1)),
    created_at INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX probes_one_builtin ON probes (builtin) WHERE builtin = 1;

INSERT INTO probes (node_id, name, builtin, created_at)
SELECT id, 'Built-in', 1, CAST(strftime('%s', 'now') AS INTEGER) * 1000
FROM nodes WHERE parent_id IS NULL;

-- One monitor on one node. `settings` is the protocol's `MonitorSettings` JSON without secret
-- values or the push token; those are sealed in `monitor_secrets` with context
-- `monitor:<id>:<field>`. `push_token_hash` is the SHA-256 of the push token, for lookups.
-- Intervals are seconds. `revision` rises with every change, so a running check of an older
-- revision is not evaluated. A group's members point at it with `group_id`.
CREATE TABLE monitors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id         INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    group_id        INTEGER REFERENCES monitors (id) ON DELETE SET NULL,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL,
    type            TEXT NOT NULL,
    settings        TEXT NOT NULL,
    interval        INTEGER NOT NULL,
    retry_interval  INTEGER NOT NULL,
    max_retries     INTEGER NOT NULL,
    resend_interval INTEGER NOT NULL,
    timeout         REAL NOT NULL,
    upside_down     INTEGER NOT NULL CHECK (upside_down IN (0, 1)),
    quorum          TEXT NOT NULL CHECK (quorum IN ('majority', 'any', 'all')),
    active          INTEGER NOT NULL CHECK (active IN (0, 1)),
    tags            TEXT NOT NULL,
    push_token_hash TEXT UNIQUE,
    manual_status   INTEGER CHECK (manual_status IN (0, 1, 2)),
    revision        INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX monitors_node ON monitors (node_id, name);
CREATE INDEX monitors_group ON monitors (group_id);

CREATE TABLE monitor_secrets (
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    field      TEXT NOT NULL,
    sealed     BLOB NOT NULL,
    PRIMARY KEY (monitor_id, field)
) STRICT, WITHOUT ROWID;

-- What the probes run changes whenever a monitor row does, including rows a node deletion
-- cascades to. The built-in probe re-reads assignments when this version moves.
CREATE TABLE assignments (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL
) STRICT;

INSERT INTO assignments (id, version) VALUES (1, 0);

CREATE TRIGGER monitors_assign_insert AFTER INSERT ON monitors
BEGIN UPDATE assignments SET version = version + 1; END;
CREATE TRIGGER monitors_assign_update AFTER UPDATE ON monitors
BEGIN UPDATE assignments SET version = version + 1; END;
CREATE TRIGGER monitors_assign_delete AFTER DELETE ON monitors
BEGIN UPDATE assignments SET version = version + 1; END;

-- The monitor-level status after retries and quorum, and the resend counter. Statuses use
-- Kuma's codes: 0 down, 1 up, 2 pending, 3 maintenance. `retries` counts failures in a row
-- for monitors the server evaluates itself (push, group, manual).
CREATE TABLE monitor_state (
    monitor_id INTEGER PRIMARY KEY REFERENCES monitors (id) ON DELETE CASCADE,
    status     INTEGER NOT NULL CHECK (status BETWEEN 0 AND 3),
    message    TEXT NOT NULL,
    latency_ms INTEGER,
    retries    INTEGER NOT NULL,
    down_count INTEGER NOT NULL,
    beat_at    INTEGER NOT NULL,
    changed_at INTEGER NOT NULL
) STRICT;

-- Each probe's status of a monitor after that probe's own retries.
CREATE TABLE probe_state (
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    probe_id   INTEGER NOT NULL REFERENCES probes (id) ON DELETE CASCADE,
    status     INTEGER NOT NULL CHECK (status BETWEEN 0 AND 3),
    message    TEXT NOT NULL,
    latency_ms INTEGER,
    retries    INTEGER NOT NULL,
    checked_at INTEGER NOT NULL,
    PRIMARY KEY (monitor_id, probe_id)
) STRICT, WITHOUT ROWID;

-- Important monitor-level beats as Kuma marks them, and repeated down alerts. `previous` is
-- NULL on a monitor's first beat. `notify` marks what Kuma would notify about; notifications
-- (milestone 07) and incidents (08) read from here.
CREATE TABLE monitor_transitions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    time       INTEGER NOT NULL,
    previous   INTEGER CHECK (previous BETWEEN 0 AND 3),
    status     INTEGER NOT NULL CHECK (status BETWEEN 0 AND 3),
    message    TEXT NOT NULL,
    important  INTEGER NOT NULL CHECK (important IN (0, 1)),
    notify     INTEGER NOT NULL CHECK (notify IN (0, 1)),
    resend     INTEGER NOT NULL CHECK (resend IN (0, 1))
) STRICT;

CREATE INDEX monitor_transitions_monitor ON monitor_transitions (monitor_id, id);
