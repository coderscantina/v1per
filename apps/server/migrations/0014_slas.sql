-- SLAs (milestone 14, decision 0012, monitoring contract "SLAs"). The whole milestone's schema
-- lands in this one migration. Every table starts empty, so an install without SLAs carries no
-- data and no work; only the notification outbox is rebuilt, to take the SLA events.

-- An SLA on a node. `start_date` is the contract start as `YYYY-MM-DD` in `timezone`.
-- `availability` is a percentage, `latency_ms` an average over up beats; absent means no
-- such commitment. `revision` rises with every change a person makes.
CREATE TABLE slas (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id      INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    timezone     TEXT NOT NULL,
    period       TEXT NOT NULL CHECK (period IN ('month', 'quarter', 'year')),
    start_date   TEXT NOT NULL,
    combine      TEXT NOT NULL CHECK (combine IN ('each', 'any_down')),
    availability REAL CHECK (availability > 0 AND availability <= 100),
    latency_ms   INTEGER CHECK (latency_ms > 0),
    revision     INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX slas_node ON slas (node_id, name);

-- The monitors an SLA covers, on its node or below. A node or monitor move drops monitors
-- that leave the subtree; deleting a monitor drops it. Frozen periods keep their numbers.
CREATE TABLE sla_monitors (
    sla_id     INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    monitor_id INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    PRIMARY KEY (sla_id, monitor_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX sla_monitors_monitor ON sla_monitors (monitor_id, sla_id);

-- Response and resolution targets in business minutes, per worst impact.
CREATE TABLE sla_time_targets (
    sla_id             INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    impact             TEXT NOT NULL CHECK (impact IN ('major_outage', 'partial_outage', 'degraded')),
    response_minutes   INTEGER CHECK (response_minutes > 0),
    resolution_minutes INTEGER CHECK (resolution_minutes > 0),
    PRIMARY KEY (sla_id, impact),
    CHECK (response_minutes IS NOT NULL OR resolution_minutes IS NOT NULL)
) STRICT, WITHOUT ROWID;

-- Weekly business hours in the SLA's timezone: minutes of the day, Sunday is weekday 0. No
-- rows: the clocks run around the clock.
CREATE TABLE sla_hours (
    sla_id       INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    weekday      INTEGER NOT NULL CHECK (weekday BETWEEN 0 AND 6),
    start_minute INTEGER NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
    end_minute   INTEGER NOT NULL CHECK (end_minute > start_minute AND end_minute <= 1440),
    PRIMARY KEY (sla_id, weekday, start_minute)
) STRICT, WITHOUT ROWID;

-- Days without business hours, `YYYY-MM-DD` in the SLA's timezone.
CREATE TABLE sla_holidays (
    sla_id INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    day    TEXT NOT NULL,
    PRIMARY KEY (sla_id, day)
) STRICT, WITHOUT ROWID;

-- The channels that get an SLA's warnings and breaches: on its node or an ancestor. A node
-- move drops links that leave that path.
CREATE TABLE sla_channels (
    sla_id     INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    channel_id INTEGER NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
    PRIMARY KEY (sla_id, channel_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX sla_channels_channel ON sla_channels (channel_id, sla_id);

-- The `any_down` service timeline per closed day in the SLA's timezone: minutes in which a
-- covered monitor had an up or down beat, and those in which one was down or pending. Written
-- once the day ends, so it outlives the minute rollups it was read from. A day without minute
-- rollups has no row.
CREATE TABLE sla_tallies (
    sla_id          INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    day             TEXT NOT NULL,
    counted_minutes INTEGER NOT NULL CHECK (counted_minutes BETWEEN 0 AND 1500),
    down_minutes    INTEGER NOT NULL CHECK (down_minutes BETWEEN 0 AND counted_minutes),
    written_at      INTEGER NOT NULL,
    PRIMARY KEY (sla_id, day)
) STRICT, WITHOUT ROWID;

-- A closed period's result as it was frozen, the protocol's `SlaPeriod` as JSON with the
-- terms it was computed with. `start` is the period's first day; `starts_at` and `ends_at`
-- the instants it covers. Only `slas.recompute` replaces a row.
CREATE TABLE sla_periods (
    sla_id    INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    start     TEXT NOT NULL,
    starts_at INTEGER NOT NULL,
    ends_at   INTEGER NOT NULL CHECK (ends_at > starts_at),
    result    TEXT NOT NULL,
    frozen_at INTEGER NOT NULL,
    PRIMARY KEY (sla_id, start)
) STRICT;

-- Alerts already queued, so each goes out once per period: `kind` is the outbox event and
-- `subject` what it is about, 0 for the whole SLA, a monitor ID for a per-monitor breach in
-- `each` mode, an incident ID for a missed response or resolution. IDs are never reused, so
-- a deleted monitor or incident cannot alert twice.
CREATE TABLE sla_alerts (
    sla_id  INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    start   TEXT NOT NULL,
    kind    TEXT NOT NULL CHECK (kind IN ('sla_budget_warning', 'sla_availability_breach',
                'sla_response_missed', 'sla_resolution_missed', 'sla_latency_breach')),
    subject INTEGER NOT NULL,
    sent_at INTEGER NOT NULL,
    PRIMARY KEY (sla_id, start, kind, subject)
) STRICT, WITHOUT ROWID;

-- The SLAs a status page shows, in order: on the page's node or below.
CREATE TABLE page_slas (
    page_id  INTEGER NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    sla_id   INTEGER NOT NULL REFERENCES slas (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (page_id, sla_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX page_slas_sla ON page_slas (sla_id);

-- The outbox takes the SLA events. SQLite cannot change a CHECK in place, so the table is
-- rebuilt with every row, ID and index as before. No table references it, so dropping the old
-- one cascades nowhere. SLA rows name their SLA in `sla_id` and leave `transition_id` empty;
-- `monitor_id` names a monitor only for a per-monitor breach. Like `monitor_id`, `sla_id` has
-- no foreign key, so the delivery log outlives a deleted SLA.
CREATE TABLE notification_outbox_v14 (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id         INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    channel_id      INTEGER REFERENCES channels (id) ON DELETE CASCADE,
    event           TEXT NOT NULL CHECK (event IN ('monitor_down', 'monitor_up',
                        'certificate_expiry', 'probe_offline', 'test', 'sla_budget_warning',
                        'sla_availability_breach', 'sla_response_missed',
                        'sla_resolution_missed', 'sla_latency_breach')),
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
    sla_id          INTEGER,
    CHECK ((state = 'pending') = (next_attempt_at IS NOT NULL))
) STRICT;

INSERT INTO notification_outbox_v14 (id, node_id, channel_id, event, monitor_id, transition_id,
    payload, summary, config, state, attempts, next_attempt_at, error, detail, created_at,
    finished_at)
SELECT id, node_id, channel_id, event, monitor_id, transition_id, payload, summary, config,
    state, attempts, next_attempt_at, error, detail, created_at, finished_at
FROM notification_outbox;

-- Delivery IDs are never reused: carry the counter over, which may be past the last row or
-- exist without any row left.
INSERT INTO sqlite_sequence (name, seq)
SELECT 'notification_outbox_v14', seq FROM sqlite_sequence
WHERE name = 'notification_outbox'
  AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'notification_outbox_v14');
UPDATE sqlite_sequence
SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'notification_outbox'), 0))
WHERE name = 'notification_outbox_v14';

DROP TABLE notification_outbox;
ALTER TABLE notification_outbox_v14 RENAME TO notification_outbox;

CREATE UNIQUE INDEX notification_outbox_once ON notification_outbox (transition_id, channel_id)
    WHERE transition_id IS NOT NULL;
CREATE INDEX notification_outbox_due ON notification_outbox (next_attempt_at)
    WHERE state = 'pending';
CREATE INDEX notification_outbox_queue ON notification_outbox (channel_id, id)
    WHERE state = 'pending';
CREATE INDEX notification_outbox_channel ON notification_outbox (channel_id, id);
CREATE INDEX notification_outbox_finished ON notification_outbox (finished_at)
    WHERE finished_at IS NOT NULL;
CREATE INDEX notification_outbox_monitor ON notification_outbox (monitor_id, id);
CREATE INDEX notification_outbox_sla ON notification_outbox (sla_id, id)
    WHERE sla_id IS NOT NULL;
