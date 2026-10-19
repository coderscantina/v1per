-- Notification channels, the delivery outbox and certificate expiry bookkeeping (milestone 07,
-- decision 0005, docs/contracts/kuma-notifications.md), plus the instance settings they read.

-- A channel on a node, usable by monitors on the node and below. `config` is Kuma's
-- `notification.config` JSON, secrets included, sealed under `channel:<id>:config`; `provider`
-- repeats its `type` for lists. Imported channels of providers v1per cannot send to are
-- inactive with a reason.
CREATE TABLE channels (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id         INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    provider        TEXT NOT NULL,
    config          BLOB NOT NULL,
    is_default      INTEGER NOT NULL CHECK (is_default IN (0, 1)),
    active          INTEGER NOT NULL CHECK (active IN (0, 1)),
    inactive_reason TEXT,
    revision        INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX channels_node ON channels (node_id, name);

-- The channels a monitor alerts: on the monitor's node or an ancestor. A node move drops
-- links that leave that path.
CREATE TABLE monitor_channels (
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    channel_id INTEGER NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
    PRIMARY KEY (monitor_id, channel_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX monitor_channels_channel ON monitor_channels (channel_id, monitor_id);

-- One message for one channel, written in the transaction of the transition that causes it,
-- and the channel's delivery log. `payload` is the event as it was then, without secrets; it
-- outlives a deleted monitor. A test of an unsaved form has no channel and carries its own
-- config sealed under `delivery:<id>:config`. Pending rows go out oldest first per channel;
-- finished rows are deleted after 30 days.
CREATE TABLE notification_outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id         INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    channel_id      INTEGER REFERENCES channels (id) ON DELETE CASCADE,
    event           TEXT NOT NULL CHECK (event IN ('monitor_down', 'monitor_up',
                        'certificate_expiry', 'probe_offline', 'test')),
    monitor_id      INTEGER,
    transition_id   INTEGER REFERENCES monitor_transitions (id) ON DELETE SET NULL,
    payload         TEXT NOT NULL,
    summary         TEXT NOT NULL,
    config          BLOB,
    state           TEXT NOT NULL CHECK (state IN ('pending', 'delivered', 'failed', 'suppressed')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER,
    error           TEXT,
    detail          TEXT,
    created_at      INTEGER NOT NULL,
    finished_at     INTEGER,
    CHECK ((state = 'pending') = (next_attempt_at IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX notification_outbox_once ON notification_outbox (transition_id, channel_id)
    WHERE transition_id IS NOT NULL;
CREATE INDEX notification_outbox_due ON notification_outbox (next_attempt_at)
    WHERE state = 'pending';
CREATE INDEX notification_outbox_queue ON notification_outbox (channel_id, id)
    WHERE state = 'pending';
CREATE INDEX notification_outbox_channel ON notification_outbox (channel_id, id);
CREATE INDEX notification_outbox_finished ON notification_outbox (finished_at)
    WHERE finished_at IS NOT NULL;

-- Certificate expiry thresholds already alerted per monitor, Kuma's
-- `notification_sent_history` rows of type `certificate`. Cleared when the server
-- certificate's fingerprint changes.
CREATE TABLE certificate_alerts (
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    days       INTEGER NOT NULL,
    PRIMARY KEY (monitor_id, days)
) STRICT, WITHOUT ROWID;

-- The SHA-256 fingerprint of the server certificate a monitor last saw.
CREATE TABLE monitor_certificates (
    monitor_id  INTEGER PRIMARY KEY REFERENCES monitors (id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL
) STRICT;

-- Kuma's `serverTimezone` and `tlsExpiryNotifyDays` settings, and the instance SMTP server.
-- The SMTP password is sealed under `instance:smtp:password`.
ALTER TABLE instance_settings ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';
ALTER TABLE instance_settings ADD COLUMN tls_expiry_notify_days TEXT NOT NULL DEFAULT '[7,14,21]';
ALTER TABLE instance_settings ADD COLUMN smtp_host TEXT;
ALTER TABLE instance_settings ADD COLUMN smtp_port INTEGER;
ALTER TABLE instance_settings ADD COLUMN smtp_secure INTEGER NOT NULL DEFAULT 0
    CHECK (smtp_secure IN (0, 1));
ALTER TABLE instance_settings ADD COLUMN smtp_ignore_tls_error INTEGER NOT NULL DEFAULT 0
    CHECK (smtp_ignore_tls_error IN (0, 1));
ALTER TABLE instance_settings ADD COLUMN smtp_ignore_starttls INTEGER NOT NULL DEFAULT 0
    CHECK (smtp_ignore_starttls IN (0, 1));
ALTER TABLE instance_settings ADD COLUMN smtp_username TEXT;
ALTER TABLE instance_settings ADD COLUMN smtp_password BLOB;
ALTER TABLE instance_settings ADD COLUMN smtp_from TEXT;
