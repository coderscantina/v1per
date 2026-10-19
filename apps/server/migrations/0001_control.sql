-- Control database: accounts, sessions, topic events and idempotency records.
-- Timestamps are Unix milliseconds. IDs are UUID strings.

CREATE TABLE users (
    id             TEXT PRIMARY KEY,
    email          TEXT NOT NULL UNIQUE,
    name           TEXT NOT NULL,
    password_hash  TEXT NOT NULL,
    -- Instance settings, SMTP, shared probes, backups and the root node (decision 0001).
    instance_admin INTEGER NOT NULL DEFAULT 0 CHECK (instance_admin IN (0, 1)),
    created_at     INTEGER NOT NULL
) STRICT;

-- `id` is the SHA-256 of the cookie token, so a leaked database does not leak live sessions.
CREATE TABLE sessions (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
) STRICT;

CREATE INDEX sessions_user ON sessions (user_id);

-- One ordered log per topic (`instance`, `user:<id>`). A command's revision is the sequence
-- of the last event it recorded.
CREATE TABLE topic_events (
    topic      TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (topic, seq)
) STRICT;

-- Committed command answers, replayable for 24 hours.
CREATE TABLE idempotency_keys (
    owner       TEXT NOT NULL,
    key         TEXT NOT NULL,
    operation   TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    response    TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (owner, key)
) STRICT;

CREATE INDEX idempotency_keys_created ON idempotency_keys (created_at);
