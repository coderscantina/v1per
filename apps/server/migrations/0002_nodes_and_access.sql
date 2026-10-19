-- Node tree, grants, invitations, second factors, API keys and instance settings (milestone 04,
-- decision 0001). Every token, key secret, invitation code and recovery code is stored as a
-- SHA-256 hex digest, as session IDs already are; TOTP secrets are sealed (decision 0006).

-- One tree of uniform nodes. The root has no parent and is created here, so `init` and data
-- directories from before this migration both have one. AUTOINCREMENT: an ID is never reused,
-- so an old cursor, API key or link never reaches a later node. Siblings are ordered by
-- `position`, 0 to n-1. Deleting a node deletes its subtree with everything that cascades.
-- Ancestors are resolved by a recursive walk up `parent_id`, at most 9 rows deep.
CREATE TABLE nodes (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id           INTEGER REFERENCES nodes (id) ON DELETE CASCADE,
    name                TEXT NOT NULL,
    position            INTEGER NOT NULL,
    -- Everyone holding a grant on this node or below it must use 2FA.
    two_factor_required INTEGER NOT NULL DEFAULT 0 CHECK (two_factor_required IN (0, 1)),
    created_at          INTEGER NOT NULL
) STRICT;

CREATE INDEX nodes_children ON nodes (parent_id, position);
CREATE UNIQUE INDEX nodes_one_root ON nodes ((parent_id IS NULL)) WHERE parent_id IS NULL;

INSERT INTO nodes (parent_id, name, position, created_at)
VALUES (NULL, 'Root', 0, CAST(strftime('%s', 'now') AS INTEGER) * 1000);

-- A role on a node and its subtree. The effective role is the highest grant on the path.
CREATE TABLE grants (
    node_id    INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (node_id, user_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX grants_user ON grants (user_id);

-- Single-use invitations to a node. The inviter receives the code once; a signed-in person or
-- a new account with `email` redeems it.
CREATE TABLE invitations (
    id         TEXT PRIMARY KEY,
    node_id    INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    email      TEXT NOT NULL,
    prefix     TEXT NOT NULL,
    code_hash  TEXT NOT NULL UNIQUE,
    created_by TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_by    TEXT REFERENCES users (id) ON DELETE SET NULL,
    used_at    INTEGER,
    revoked_at INTEGER
) STRICT;

CREATE INDEX invitations_node ON invitations (node_id, created_at);

-- The last passed password check of a login session; sensitive commands need it within 10
-- minutes.
ALTER TABLE sessions ADD COLUMN reauthenticated_at INTEGER;

-- The step between a correct password and the second factor. `id` is the digest of the
-- challenge cookie. Never a session: it reaches only `auth.verifySecondFactor`.
CREATE TABLE login_challenges (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    attempts   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX login_challenges_user ON login_challenges (user_id);

-- One second factor per person, sealed under the secrets key with context
-- `user:<id>:totp`. `pending_secret` waits for `twoFactor.confirm` and leaves an active factor
-- working meanwhile. `last_step` is the newest accepted time step, so a code works once.
CREATE TABLE two_factor (
    user_id            TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret             BLOB,
    confirmed_at       INTEGER,
    last_step          INTEGER,
    pending_secret     BLOB,
    pending_created_at INTEGER,
    CHECK ((secret IS NULL) = (confirmed_at IS NULL)),
    CHECK ((pending_secret IS NULL) = (pending_created_at IS NULL))
) STRICT;

-- Single-use recovery codes. `used_at` marks a spent code; regeneration replaces the set.
CREATE TABLE recovery_codes (
    user_id   TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at   INTEGER,
    PRIMARY KEY (user_id, code_hash)
) STRICT, WITHOUT ROWID;

-- Scoped API keys: one node and its subtree, a set of capabilities (JSON array of names), and
-- never more than the owner currently holds there. A deleted node takes its keys along.
CREATE TABLE api_keys (
    id           TEXT PRIMARY KEY,
    owner_id     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    node_id      INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    prefix       TEXT NOT NULL,
    secret_hash  TEXT NOT NULL UNIQUE,
    capabilities TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at   INTEGER
) STRICT;

CREATE INDEX api_keys_owner ON api_keys (owner_id);

-- Settings the instance admin changes in the app. `V1PER_INSTANCE_NAME` and
-- `V1PER_PUBLIC_URL` win over the stored values while set. SMTP follows in milestone 07.
CREATE TABLE instance_settings (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    name       TEXT,
    public_url TEXT,
    updated_at INTEGER NOT NULL
) STRICT;

INSERT INTO instance_settings (id, updated_at) VALUES (1, 0);
