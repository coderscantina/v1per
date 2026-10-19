-- Remote probe agents (milestone 06, decision 0003): registration, health, which probes check
-- which monitor, and the probe journal. Control database only; heartbeats and pending inbox
-- batches are untouched.

-- `token_hash` is the SHA-256 of the agent's token, shown once at registration. The health
-- columns hold what the agent reported at its last contact. `offline_since` is set after 90
-- seconds without contact and cleared at the next one.
ALTER TABLE probes ADD COLUMN token_hash TEXT;
ALTER TABLE probes ADD COLUMN revoked_at INTEGER;
ALTER TABLE probes ADD COLUMN last_seen_at INTEGER;
ALTER TABLE probes ADD COLUMN offline_since INTEGER;
ALTER TABLE probes ADD COLUMN version TEXT;
ALTER TABLE probes ADD COLUMN clock_skew_ms INTEGER;
ALTER TABLE probes ADD COLUMN checks_started INTEGER NOT NULL DEFAULT 0;
ALTER TABLE probes ADD COLUMN checks_late INTEGER NOT NULL DEFAULT 0;
ALTER TABLE probes ADD COLUMN buffered_results INTEGER NOT NULL DEFAULT 0;
ALTER TABLE probes ADD COLUMN dropped_results INTEGER NOT NULL DEFAULT 0;

CREATE UNIQUE INDEX probes_token ON probes (token_hash) WHERE token_hash IS NOT NULL;
CREATE INDEX probes_node ON probes (node_id, id);

-- The probes that check a monitor. Only monitors a probe runs have rows; push, group and
-- manual monitors run on the server. A probe serves monitors on its node and below; rows that
-- a node move takes out of that subtree are deleted with the move.
CREATE TABLE monitor_probes (
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    probe_id   INTEGER NOT NULL REFERENCES probes (id) ON DELETE CASCADE,
    PRIMARY KEY (monitor_id, probe_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX monitor_probes_probe ON monitor_probes (probe_id, monitor_id);

-- Until now the built-in probe ran every monitor.
INSERT INTO monitor_probes (monitor_id, probe_id)
SELECT m.id, p.id FROM monitors m, probes p
WHERE p.builtin = 1 AND m.type NOT IN ('push', 'group', 'manual', 'unsupported');

CREATE TRIGGER monitor_probes_assign_insert AFTER INSERT ON monitor_probes
BEGIN UPDATE assignments SET version = version + 1; END;
CREATE TRIGGER monitor_probes_assign_delete AFTER DELETE ON monitor_probes
BEGIN UPDATE assignments SET version = version + 1; END;
CREATE TRIGGER probes_assign_revoke AFTER UPDATE OF revoked_at ON probes
BEGIN UPDATE assignments SET version = version + 1; END;

-- When the server received the probe's latest result, on the server's clock; `checked_at` is
-- the probe's own time. Liveness for the quorum is read from `received_at`.
ALTER TABLE probe_state ADD COLUMN received_at INTEGER NOT NULL DEFAULT 0;
UPDATE probe_state SET received_at = checked_at;

-- Set while none of the monitor's probes is live; the monitor is then pending.
ALTER TABLE monitor_state ADD COLUMN no_live_probe INTEGER NOT NULL DEFAULT 0
    CHECK (no_live_probe IN (0, 1));

-- Probes going offline and coming back. Notifications (milestone 07) read from here.
CREATE TABLE probe_transitions (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    probe_id INTEGER NOT NULL REFERENCES probes (id) ON DELETE CASCADE,
    time     INTEGER NOT NULL,
    online   INTEGER NOT NULL CHECK (online IN (0, 1))
) STRICT;

CREATE INDEX probe_transitions_probe ON probe_transitions (probe_id, id);
