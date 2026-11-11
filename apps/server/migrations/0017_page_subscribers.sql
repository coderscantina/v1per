-- Page subscriptions (docs/contracts/status-page-api.md, Subscriptions): visitors who follow a
-- page by email, webhooks its managers add, and the queue that tells them.

-- An address that follows a page, with only what sending needs. `components` is a JSON array
-- of page component IDs, empty for the whole page. `token_hash` is the SHA-256 of the secret in
-- the subscriber's links; the secret is derived from the instance key, the page, the address and
-- `created_at`, so each mail can carry it again. Unconfirmed rows end after 24 hours.
CREATE TABLE page_subscribers (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id      INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    email        TEXT NOT NULL,
    locale       TEXT NOT NULL,
    components   TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    confirmed_at INTEGER,
    created_at   INTEGER NOT NULL,
    UNIQUE (page_id, email)
) STRICT;

CREATE INDEX page_subscribers_unconfirmed ON page_subscribers (created_at)
    WHERE confirmed_at IS NULL;

-- A URL a page posts to. `url` is sealed under `page_webhook:<id>:url`, since it may carry a
-- token; `host` is what lists show.
CREATE TABLE page_webhooks (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id    INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    url        BLOB NOT NULL,
    host       TEXT NOT NULL,
    components TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX page_webhooks_page ON page_webhooks (page_id);

-- Something a page's subscribers hear about, written in the transaction that caused it.
-- `content` is the incident or maintenance as the public page showed it then, per locale;
-- `components` the page component IDs it names, for the subscribers' selection.
CREATE TABLE page_notices (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id     INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    event       TEXT NOT NULL CHECK (event IN ('incident_published', 'incident_updated',
                    'incident_resolved', 'maintenance_scheduled', 'maintenance_started')),
    incident_id INTEGER REFERENCES incidents (id) ON DELETE CASCADE,
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL
) STRICT;

-- One notice for one subscriber or webhook, until it is sent or fails for good. Retried like
-- the notification outbox.
CREATE TABLE page_deliveries (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    notice_id       INTEGER NOT NULL REFERENCES page_notices (id) ON DELETE CASCADE,
    subscriber_id   INTEGER REFERENCES page_subscribers (id) ON DELETE CASCADE,
    webhook_id      INTEGER REFERENCES page_webhooks (id) ON DELETE CASCADE,
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    CHECK ((subscriber_id IS NULL) != (webhook_id IS NULL))
) STRICT;

CREATE INDEX page_deliveries_due ON page_deliveries (next_attempt_at);
CREATE INDEX page_deliveries_notice ON page_deliveries (notice_id);
CREATE INDEX page_deliveries_subscriber ON page_deliveries (subscriber_id, id);
CREATE INDEX page_deliveries_webhook ON page_deliveries (webhook_id, id);

-- Maintenance windows a page already announced, so each is told once when it shows and once
-- when it starts. `starts_at` is the window's start, 0 for a manual maintenance.
CREATE TABLE page_maintenance_notices (
    page_id        INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    maintenance_id INTEGER NOT NULL REFERENCES maintenances (id) ON DELETE CASCADE,
    starts_at      INTEGER NOT NULL,
    event          TEXT NOT NULL CHECK (event IN ('maintenance_scheduled', 'maintenance_started')),
    PRIMARY KEY (page_id, maintenance_id, starts_at, event)
) STRICT, WITHOUT ROWID;
