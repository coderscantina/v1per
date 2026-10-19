-- Status pages (milestone 09, decision 0004).

-- A page on a node. `locales` is a JSON list of enabled locales. Access: `public`, or
-- restricted with whichever modes are set: `password_hash` (argon2), `allowed_emails` and
-- `allowed_domains` (JSON lists) for sign-in links, and share tokens. Grant holders on the
-- node always get in. `revision` rises with every change a person makes to the page.
CREATE TABLE pages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id         INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    slug            TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    description     TEXT NOT NULL,
    footer          TEXT NOT NULL,
    theme           TEXT NOT NULL CHECK (theme IN ('auto', 'light', 'dark')),
    accent          TEXT NOT NULL,
    timezone        TEXT NOT NULL,
    default_locale  TEXT NOT NULL CHECK (default_locale IN ('en', 'de')),
    locales         TEXT NOT NULL,
    auto_incidents  TEXT NOT NULL CHECK (auto_incidents IN ('draft', 'auto_publish')),
    grace_minutes   INTEGER NOT NULL CHECK (grace_minutes >= 0),
    public          INTEGER NOT NULL CHECK (public IN (0, 1)),
    password_hash   TEXT,
    allowed_emails  TEXT NOT NULL DEFAULT '[]',
    allowed_domains TEXT NOT NULL DEFAULT '[]',
    logo            BLOB,
    logo_type       TEXT,
    logo_updated_at INTEGER,
    revision        INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    CHECK ((logo IS NULL) = (logo_type IS NULL))
) STRICT;

CREATE INDEX pages_node ON pages (node_id, title);

CREATE TABLE page_groups (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id      INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    name         TEXT NOT NULL,
    default_open INTEGER NOT NULL CHECK (default_open IN (0, 1))
) STRICT;

CREATE INDEX page_groups_page ON page_groups (page_id, position);

-- A monitor or static component placed on a page, each at most once per page. Deleting the
-- monitor or component takes it off; a node move takes off targets that left the subtree.
CREATE TABLE page_components (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id      INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    group_id     INTEGER NOT NULL REFERENCES page_groups (id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    monitor_id   INTEGER REFERENCES monitors (id) ON DELETE CASCADE,
    component_id INTEGER REFERENCES components (id) ON DELETE CASCADE,
    display_name TEXT,
    internal     INTEGER NOT NULL CHECK (internal IN (0, 1)),
    CHECK ((monitor_id IS NULL) != (component_id IS NULL))
) STRICT;

CREATE INDEX page_components_page ON page_components (page_id, group_id, position);
CREATE UNIQUE INDEX page_components_monitor ON page_components (monitor_id, page_id)
    WHERE monitor_id IS NOT NULL;
CREATE UNIQUE INDEX page_components_component ON page_components (component_id, page_id)
    WHERE component_id IS NOT NULL;

-- Hostnames routed to a page by the request's `Host`, lowercase without port.
CREATE TABLE page_hostnames (
    hostname   TEXT PRIMARY KEY,
    page_id    INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX page_hostnames_page ON page_hostnames (page_id);

-- `token_hash` is the SHA-256 of the token; the token itself is shown once.
CREATE TABLE page_share_tokens (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id    INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at INTEGER,
    created_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX page_share_tokens_page ON page_share_tokens (page_id);

-- A visitor's access to one page, behind the page cookie. `id` is the SHA-256 of the cookie.
-- A password session ends when the password changes, a token session with its token; an
-- email session holds only while its address stays allowed.
CREATE TABLE page_sessions (
    id             TEXT PRIMARY KEY,
    page_id        INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('password', 'email', 'token')),
    email          TEXT,
    share_token_id INTEGER REFERENCES page_share_tokens (id) ON DELETE CASCADE,
    expires_at     INTEGER NOT NULL,
    created_at     INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX page_sessions_page ON page_sessions (page_id, kind);

-- Emailed sign-in links, single use. `id` is the SHA-256 of the code.
CREATE TABLE page_email_codes (
    id         TEXT PRIMARY KEY,
    page_id    INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    email      TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- Milestone 08 created this without pages to point at; nothing wrote it yet.
DROP TABLE incident_publications;

CREATE TABLE incident_publications (
    incident_id  INTEGER NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    page_id      INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    published_at INTEGER NOT NULL,
    PRIMARY KEY (incident_id, page_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX incident_publications_page ON incident_publications (page_id, incident_id);

-- Auto-incidents waiting out a page's grace before `auto_publish` publishes them. Rows go
-- when they are published, unpublished by a person, or the incident resolves first.
CREATE TABLE incident_auto_publications (
    incident_id INTEGER NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    page_id     INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    due_at      INTEGER NOT NULL,
    PRIMARY KEY (incident_id, page_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX incident_auto_publications_due ON incident_auto_publications (due_at);

-- What generated an auto-incident's title, so pages render it in their locale with the name
-- they show. NULL once a person changes the title, and for manual incidents. Auto-incidents
-- whose stored English title is still the generated one get the kind back.
ALTER TABLE incidents ADD COLUMN title_kind TEXT CHECK (title_kind IN ('monitor_down'));

UPDATE incidents SET title_kind = 'monitor_down'
WHERE auto = 1
  AND title = (SELECT m.name || ' is down' FROM monitors m WHERE m.id = incidents.auto_monitor_id);
