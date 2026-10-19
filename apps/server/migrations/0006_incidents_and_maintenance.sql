-- Maintenance, static components and incidents (milestone 08, decisions 0004 and 0008).

-- A maintenance on a node, with Kuma's `maintenance` columns that decide its windows. Dates
-- are local `YYYY-MM-DD HH:MM:SS` in `timezone` (NULL: the instance timezone), times `HH:MM`;
-- `weekdays` and `days_of_month` are Kuma's JSON lists. `status` is the status the server
-- last applied, so a window starting or ending is announced once.
CREATE TABLE maintenances (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id       INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    description   TEXT NOT NULL,
    strategy      TEXT NOT NULL CHECK (strategy IN ('manual', 'single', 'recurring-interval',
                      'recurring-weekday', 'recurring-day-of-month', 'cron')),
    active        INTEGER NOT NULL CHECK (active IN (0, 1)),
    start_date    TEXT,
    end_date      TEXT,
    start_time    TEXT,
    end_time      TEXT,
    weekdays      TEXT NOT NULL DEFAULT '[]',
    days_of_month TEXT NOT NULL DEFAULT '[]',
    interval_day  INTEGER,
    cron          TEXT,
    duration      INTEGER,
    timezone      TEXT,
    status        TEXT NOT NULL DEFAULT 'scheduled',
    revision      INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

CREATE INDEX maintenances_node ON maintenances (node_id, title);

-- The monitors a maintenance covers, on its node or below. A node move drops links that
-- leave the subtree.
CREATE TABLE maintenance_monitors (
    maintenance_id INTEGER NOT NULL REFERENCES maintenances (id) ON DELETE CASCADE,
    monitor_id     INTEGER NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    PRIMARY KEY (maintenance_id, monitor_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX maintenance_monitors_monitor ON maintenance_monitors (monitor_id);

-- Monitors under maintenance now: covered by an active window, or members of a group that is.
-- Probes do not run them; the built-in probe records maintenance beats instead.
CREATE TABLE maintenance_state (
    monitor_id INTEGER PRIMARY KEY REFERENCES monitors (id) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER maintenance_state_assign_insert AFTER INSERT ON maintenance_state
BEGIN UPDATE assignments SET version = version + 1; END;
CREATE TRIGGER maintenance_state_assign_delete AFTER DELETE ON maintenance_state
BEGIN UPDATE assignments SET version = version + 1; END;

-- A part of a service without a monitor, which incidents name and pages show.
CREATE TABLE components (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id     INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX components_node ON components (node_id, name);

-- An incident on a node. `state` and `resolved_at` follow the latest update. An auto-incident
-- names the monitor that opened it until that monitor is deleted; at most one is open per
-- monitor. `edited` marks an auto-incident a person changed, which recovery then leaves open.
CREATE TABLE incidents (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id         INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    state           TEXT NOT NULL CHECK (state IN ('investigating', 'identified', 'monitoring',
                        'resolved')),
    auto            INTEGER NOT NULL CHECK (auto IN (0, 1)),
    auto_monitor_id INTEGER REFERENCES monitors (id) ON DELETE SET NULL,
    edited          INTEGER NOT NULL DEFAULT 0 CHECK (edited IN (0, 1)),
    started_at      INTEGER NOT NULL,
    resolved_at     INTEGER,
    revision        INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    CHECK ((state = 'resolved') = (resolved_at IS NOT NULL))
) STRICT;

CREATE INDEX incidents_node ON incidents (node_id, id);
CREATE UNIQUE INDEX incidents_one_open_auto ON incidents (auto_monitor_id)
    WHERE auto_monitor_id IS NOT NULL AND resolved_at IS NULL;

-- The monitors and static components an incident affects, each once.
CREATE TABLE incident_targets (
    incident_id  INTEGER NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    monitor_id   INTEGER REFERENCES monitors (id) ON DELETE CASCADE,
    component_id INTEGER REFERENCES components (id) ON DELETE CASCADE,
    CHECK ((monitor_id IS NULL) != (component_id IS NULL))
) STRICT;

CREATE UNIQUE INDEX incident_targets_monitor ON incident_targets (incident_id, monitor_id)
    WHERE monitor_id IS NOT NULL;
CREATE UNIQUE INDEX incident_targets_component ON incident_targets (incident_id, component_id)
    WHERE component_id IS NOT NULL;
CREATE INDEX incident_targets_by_monitor ON incident_targets (monitor_id)
    WHERE monitor_id IS NOT NULL;

-- The timeline. `author_id` is NULL for what the server wrote for an auto-incident.
CREATE TABLE incident_updates (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    incident_id INTEGER NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    state       TEXT NOT NULL CHECK (state IN ('investigating', 'identified', 'monitoring',
                    'resolved')),
    message     TEXT NOT NULL,
    author_id   TEXT REFERENCES users (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX incident_updates_incident ON incident_updates (incident_id, id);

-- The impact an update sets per target.
CREATE TABLE incident_update_impacts (
    update_id    INTEGER NOT NULL REFERENCES incident_updates (id) ON DELETE CASCADE,
    monitor_id   INTEGER REFERENCES monitors (id) ON DELETE CASCADE,
    component_id INTEGER REFERENCES components (id) ON DELETE CASCADE,
    impact       TEXT NOT NULL CHECK (impact IN ('operational', 'degraded', 'partial_outage',
                     'major_outage')),
    CHECK ((monitor_id IS NULL) != (component_id IS NULL))
) STRICT;

CREATE INDEX incident_update_impacts_update ON incident_update_impacts (update_id);

-- The status pages an incident is published to. Pages arrive in milestone 09, which ties
-- `page_id` to its pages table and adds the publication rules.
CREATE TABLE incident_publications (
    incident_id  INTEGER NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    page_id      INTEGER NOT NULL,
    published_at INTEGER NOT NULL,
    PRIMARY KEY (incident_id, page_id)
) STRICT, WITHOUT ROWID;
