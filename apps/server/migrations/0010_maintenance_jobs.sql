-- Operator work that needs the engines a running server holds (milestone 11, decision 0010):
-- backups, retention runs and diagnostics. `v1per backup`, `retention` and `diagnostics` queue
-- a row; the server that holds the data directory runs it and stores the outcome, and without
-- a server the command runs it itself. `args` and `result` are JSON. Finished rows are
-- deleted by retention after 30 days.
CREATE TABLE maintenance_jobs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL CHECK (kind IN ('retention', 'retention_dry_run', 'backup', 'diagnostics')),
    args        TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed')),
    result      TEXT,
    created_at  INTEGER NOT NULL,
    started_at  INTEGER,
    finished_at INTEGER
) STRICT;

CREATE INDEX maintenance_jobs_open ON maintenance_jobs (id) WHERE state IN ('queued', 'running');
CREATE INDEX maintenance_jobs_kind ON maintenance_jobs (kind, finished_at);
