-- Uptime Kuma 2.5.0 SQLite schema (commit d9a60dfc73140d15111752e4e8910ed4b54bd9a3).
--
-- Taken with `sqlite3 kuma.db .schema` from a production Kuma 2.5.0 database that was
-- created from the template `db/kuma.db`, patched by the legacy SQL patches and upgraded
-- through all 54 knex migrations listed at the end of this file. Schema only, no data.
--
-- Column order reflects the upgrade path of that database. A fresh 2.5.0 install can order
-- `monitor` columns differently, so importers must match columns by name, not position.
--
-- `sqlite_sequence` and `sqlite_stat1` are SQLite internal tables: the first appears with
-- the first AUTOINCREMENT table, the second after `ANALYZE` or `PRAGMA optimize`, which
-- Kuma's daily retention job runs. The fixture generator skips their CREATE statements.
--
-- File format of the source: page_size 4096, auto_vacuum INCREMENTAL, journal_mode WAL,
-- encoding UTF-8.

CREATE TABLE [user](
  [id] INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, 
  [username] VARCHAR(255) NOT NULL UNIQUE, 
  [password] VARCHAR(255), 
  [active] BOOLEAN NOT NULL DEFAULT 1, 
  [timezone] VARCHAR(150), twofa_secret VARCHAR(64), twofa_status BOOLEAN default 0 NOT NULL, twofa_last_token VARCHAR(6));
CREATE TABLE sqlite_sequence(name,seq);
CREATE TABLE [notification](
  [id] INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, 
  [name] VARCHAR(255), 
  [active] BOOLEAN NOT NULL DEFAULT 1, 
  [user_id] INTEGER NOT NULL, is_default BOOLEAN default 0 NOT NULL, config TEXT);
CREATE TABLE [monitor_notification](
  [id] INTEGER PRIMARY KEY NOT NULL, 
  [monitor_id] INTEGER NOT NULL REFERENCES [monitor]([id]) ON DELETE CASCADE ON UPDATE CASCADE, 
  [notification_id] INTEGER NOT NULL REFERENCES [notification]([id]) ON DELETE CASCADE ON UPDATE CASCADE);
CREATE TABLE tag (
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	name VARCHAR(255) NOT NULL,
    color VARCHAR(255) NOT NULL,
	created_date DATETIME DEFAULT (DATETIME('now')) NOT NULL
);
CREATE TABLE monitor_tag (
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	monitor_id INTEGER NOT NULL,
	tag_id INTEGER NOT NULL,
	value TEXT,
	CONSTRAINT FK_tag FOREIGN KEY (tag_id) REFERENCES tag(id) ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT FK_monitor FOREIGN KEY (monitor_id) REFERENCES monitor(id) ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE TABLE "setting"
(
    id INTEGER
        primary key autoincrement,
    key VARCHAR(200) not null
        unique,
    value TEXT,
    type VARCHAR(20)
);
CREATE TABLE incident
(
    id INTEGER not null
        constraint incident_pk
            primary key autoincrement,
    title VARCHAR(255) not null,
    content TEXT not null,
    style VARCHAR(30) default 'warning' not null,
    created_date DATETIME default (DATETIME('now')) not null,
    last_updated_date DATETIME,
    pin BOOLEAN default 1 not null,
    active BOOLEAN default 1 not null
, status_page_id INTEGER);
CREATE TABLE `group`
(
    id           INTEGER      not null
        constraint group_pk
            primary key autoincrement,
    name         VARCHAR(255) not null,
    created_date DATETIME              default (DATETIME('now')) not null,
    public       BOOLEAN               default 0 not null,
    active       BOOLEAN               default 1 not null,
    weight       BOOLEAN      NOT NULL DEFAULT 1000
, status_page_id INTEGER);
CREATE TABLE [monitor_group]
(
    [id]         INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    [monitor_id] INTEGER                           NOT NULL REFERENCES [monitor] ([id]) ON DELETE CASCADE ON UPDATE CASCADE,
    [group_id]   INTEGER                           NOT NULL REFERENCES [group] ([id]) ON DELETE CASCADE ON UPDATE CASCADE,
    weight BOOLEAN NOT NULL DEFAULT 1000
, send_url BOOLEAN DEFAULT 0 NOT NULL, `custom_url` text);
CREATE TABLE [notification_sent_history] (
    [id] INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    [type] VARCHAR(50) NOT NULL,
    [monitor_id] INTEGER NOT NULL,
    [days] INTEGER NOT NULL,
    UNIQUE([type], [monitor_id], [days])
);
CREATE TABLE docker_host (
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	user_id INT NOT NULL,
	docker_daemon VARCHAR(255),
	docker_type VARCHAR(255),
	name VARCHAR(255)
);
CREATE TABLE [status_page_cname](
    [id] INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    [status_page_id] INTEGER NOT NULL REFERENCES [status_page]([id]) ON DELETE CASCADE ON UPDATE CASCADE,
    [domain] VARCHAR NOT NULL UNIQUE
);
CREATE TABLE [maintenance] (
    [id] INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    [title] VARCHAR(150) NOT NULL,
    [description] TEXT NOT NULL,
    [user_id] INTEGER REFERENCES [user]([id]) ON DELETE SET NULL ON UPDATE CASCADE,
    [active] BOOLEAN NOT NULL DEFAULT 1,
    [strategy] VARCHAR(50) NOT NULL DEFAULT 'single',
    [start_date] DATETIME,
    [end_date] DATETIME,
    [start_time] TIME,
    [end_time] TIME,
    [weekdays] VARCHAR2(250) DEFAULT '[]',
    [days_of_month] TEXT DEFAULT '[]',
    [interval_day] INTEGER
, cron TEXT, timezone VARCHAR(255), duration INTEGER, `last_start_date` datetime);
CREATE TABLE maintenance_status_page (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    status_page_id INTEGER NOT NULL,
    maintenance_id INTEGER NOT NULL,
    CONSTRAINT FK_maintenance FOREIGN KEY (maintenance_id) REFERENCES maintenance (id) ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT FK_status_page FOREIGN KEY (status_page_id) REFERENCES status_page (id) ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE TABLE monitor_maintenance (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    monitor_id INTEGER NOT NULL,
    maintenance_id INTEGER NOT NULL,
    CONSTRAINT FK_maintenance FOREIGN KEY (maintenance_id) REFERENCES maintenance (id) ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT FK_monitor FOREIGN KEY (monitor_id) REFERENCES monitor (id) ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE TABLE [api_key] (
    [id] INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    [key] VARCHAR(255) NOT NULL,
    [name] VARCHAR(255) NOT NULL,
    [user_id] INTEGER NOT NULL,
    [created_date] DATETIME DEFAULT (DATETIME('now')) NOT NULL,
    [active] BOOLEAN DEFAULT 1 NOT NULL,
    [expires] DATETIME DEFAULT NULL,
    CONSTRAINT FK_user FOREIGN KEY ([user_id]) REFERENCES [user]([id]) ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE TABLE sqlite_stat1(tbl,idx,stat);
CREATE TABLE `knex_migrations` (`id` integer not null primary key autoincrement, `name` varchar(255), `batch` integer, `migration_time` datetime);
CREATE TABLE `knex_migrations_lock` (`index` integer not null primary key autoincrement, `is_locked` integer);
CREATE TABLE `remote_browser` (`id` integer not null primary key autoincrement, `name` varchar(255) not null, `url` varchar(255) not null, `user_id` integer);
CREATE TABLE "monitor_tls_info" (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `monitor_id` INTEGER NOT NULL REFERENCES `monitor` (`id`) ON DELETE CASCADE ON UPDATE CASCADE, `info_json` text);
CREATE TABLE "proxy" (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `user_id` INT NOT NULL, `protocol` VARCHAR(10) NOT NULL, `host` VARCHAR(255) NOT NULL, `port` integer, `auth` BOOLEAN NOT NULL, `username` VARCHAR(255) NULL, `password` VARCHAR(255) NULL, `active` BOOLEAN NOT NULL DEFAULT 1, `default` BOOLEAN NOT NULL DEFAULT 0, `created_date` DATETIME NOT NULL DEFAULT (DATETIME('now')));
CREATE TABLE "domain_expiry" (`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL, `last_check` datetime, `domain` varchar(255) NOT NULL, `expiry` datetime, `last_expiry_notification_sent` integer DEFAULT null);
CREATE TABLE "heartbeat" (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `important` BOOLEAN NOT NULL DEFAULT 0, `monitor_id` INTEGER NOT NULL REFERENCES `monitor` (`id`) ON DELETE CASCADE ON UPDATE CASCADE, `status` SMALLINT NOT NULL, `msg` TEXT, `time` DATETIME NOT NULL, `ping` bigint, `duration` INTEGER NOT NULL DEFAULT 0, `down_count` INTEGER NOT NULL DEFAULT 0, `end_time` datetime NULL DEFAULT null, `retries` integer NOT NULL DEFAULT '0', `response` text NULL DEFAULT null);
CREATE TABLE "stat_minutely" (`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL, `monitor_id` integer NOT NULL, `timestamp` integer NOT NULL, `ping` float NOT NULL, `up` integer NOT NULL, `down` integer NOT NULL, `ping_min` float NOT NULL DEFAULT '0', `ping_max` float NOT NULL DEFAULT '0', `extras` text DEFAULT null, FOREIGN KEY (`monitor_id`) REFERENCES `monitor` (`id`) ON DELETE CASCADE ON UPDATE CASCADE);
CREATE TABLE "stat_hourly" (`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL, `monitor_id` integer NOT NULL, `timestamp` integer NOT NULL, `ping` float NOT NULL, `ping_min` float NOT NULL DEFAULT '0', `ping_max` float NOT NULL DEFAULT '0', `up` integer NOT NULL, `down` integer NOT NULL, `extras` text DEFAULT null, FOREIGN KEY (`monitor_id`) REFERENCES `monitor` (`id`) ON DELETE CASCADE ON UPDATE CASCADE);
CREATE TABLE "monitor" (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `name` VARCHAR(150), `active` BOOLEAN NOT NULL DEFAULT 1, `user_id` INTEGER REFERENCES `user` ON DELETE SET NULL ON UPDATE CASCADE, `interval` INTEGER NOT NULL DEFAULT 20, `url` TEXT, `type` VARCHAR(20), `weight` INTEGER DEFAULT 2000, `hostname` VARCHAR(255), `port` INTEGER, `created_date` DATETIME NOT NULL DEFAULT (DATETIME('now')), `keyword` VARCHAR(255), `maxretries` INTEGER NOT NULL DEFAULT 0, `ignore_tls` BOOLEAN NOT NULL DEFAULT 0, `upside_down` BOOLEAN NOT NULL DEFAULT 0, `maxredirects` INTEGER NOT NULL DEFAULT 10, `accepted_statuscodes_json` TEXT NOT NULL DEFAULT '["200-299"]', `dns_resolve_type` VARCHAR(5), `dns_resolve_server` VARCHAR(255), `dns_last_result` text, `retry_interval` INTEGER NOT NULL DEFAULT 0, `push_token` varchar(32), `method` TEXT NOT NULL DEFAULT 'GET', `body` TEXT DEFAULT null, `headers` TEXT DEFAULT null, `basic_auth_user` TEXT DEFAULT null, `basic_auth_pass` TEXT DEFAULT null, `docker_host` INTEGER REFERENCES `docker_host` (`id`), `docker_container` VARCHAR(255), `proxy_id` INTEGER REFERENCES `proxy` (`id`), `expiry_notification` BOOLEAN DEFAULT 1, `mqtt_topic` TEXT, `mqtt_success_message` VARCHAR(255), `mqtt_username` VARCHAR(255), `mqtt_password` VARCHAR(255), `database_connection_string` VARCHAR(2000), `database_query` TEXT, `auth_method` VARCHAR(250), `auth_domain` TEXT, `auth_workstation` TEXT, `grpc_url` VARCHAR(255) DEFAULT null, `grpc_protobuf` TEXT DEFAULT null, `grpc_body` TEXT DEFAULT null, `grpc_metadata` TEXT DEFAULT null, `grpc_method` VARCHAR(255) DEFAULT null, `grpc_service_name` VARCHAR(255) DEFAULT null, `grpc_enable_tls` BOOLEAN NOT NULL DEFAULT 0, `radius_username` VARCHAR(255), `radius_password` VARCHAR(255), `radius_calling_station_id` VARCHAR(50), `radius_called_station_id` VARCHAR(50), `radius_secret` VARCHAR(255), `resend_interval` INTEGER NOT NULL DEFAULT 0, `packet_size` INTEGER NOT NULL DEFAULT 56, `game` VARCHAR(255), `http_body_encoding` VARCHAR(25), `description` TEXT DEFAULT null, `tls_ca` TEXT DEFAULT null, `tls_cert` TEXT DEFAULT null, `tls_key` TEXT DEFAULT null, `parent` INTEGER REFERENCES `monitor` (`id`) ON DELETE SET NULL ON UPDATE CASCADE, `invert_keyword` BOOLEAN NOT NULL DEFAULT 0, `json_path` TEXT, `expected_value` VARCHAR(255), `kafka_producer_topic` VARCHAR(255), `kafka_producer_brokers` TEXT, `kafka_producer_sasl_options` TEXT, `kafka_producer_message` TEXT, `oauth_client_id` TEXT DEFAULT null, `oauth_client_secret` TEXT DEFAULT null, `oauth_token_url` TEXT DEFAULT null, `oauth_scopes` TEXT DEFAULT null, `oauth_auth_method` TEXT DEFAULT null, `timeout` DOUBLE NOT NULL DEFAULT 0, `gamedig_given_port_only` BOOLEAN NOT NULL DEFAULT 1, `kafka_producer_ssl` BOOLEAN NOT NULL DEFAULT 0, `kafka_producer_allow_auto_topic_creation` BOOLEAN NOT NULL DEFAULT 0, `mqtt_check_type` varchar(255) NOT NULL DEFAULT 'keyword', `remote_browser` integer NULL DEFAULT null, `snmp_oid` varchar(255) DEFAULT null, `snmp_version` text CHECK (`snmp_version` in('1' , '2c' , '3')) DEFAULT '2c', `json_path_operator` varchar(255) DEFAULT null, `cache_bust` boolean NOT NULL DEFAULT '0', `conditions` text NOT NULL DEFAULT '[]', `rabbitmq_nodes` text, `rabbitmq_username` varchar(255), `rabbitmq_password` varchar(255), `smtp_security` varchar(255) DEFAULT null, `ping_count` integer NOT NULL DEFAULT '1', `ping_numeric` boolean NOT NULL DEFAULT '1', `ping_per_request_timeout` integer NOT NULL DEFAULT '2', `ip_family` varchar(4) DEFAULT null, `manual_status` integer, `oauth_audience` varchar(255) NULL DEFAULT null, `mqtt_websocket_path` varchar(255) NULL, `ws_ignore_sec_websocket_accept_header` boolean NOT NULL DEFAULT '0', `ws_subprotocol` varchar(255) NOT NULL DEFAULT '', `domain_expiry_notification` boolean DEFAULT '0', `save_response` boolean NOT NULL DEFAULT '0', `save_error_response` boolean NOT NULL DEFAULT '1', `response_max_length` integer NOT NULL DEFAULT '1024', `system_service_name` varchar(255), `subtype` varchar(10) NULL, `location` varchar(255) NULL, `protocol` varchar(20) NULL, `snmp_v3_username` varchar(255), `expected_tls_alert` varchar(50) DEFAULT null, `retry_only_on_status_code_failure` boolean NOT NULL DEFAULT '0', `screenshot_delay` integer NOT NULL DEFAULT '0', `ntp_stratum_threshold` integer default '5', `ntp_time_offset_threshold` integer default '1000', `ntp_root_dispersion_threshold` integer default '500', `bearer_token` text default null, `gamedig_token` text default null, FOREIGN KEY (`remote_browser`) REFERENCES `remote_browser` (`id`));
CREATE TABLE "status_page" (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `slug` VARCHAR(255) NOT NULL UNIQUE, `title` VARCHAR(255) NOT NULL, `description` TEXT, `icon` VARCHAR(255) NOT NULL, `theme` VARCHAR(30) NOT NULL, `published` BOOLEAN NOT NULL DEFAULT 1, `search_engine_index` BOOLEAN NOT NULL DEFAULT 1, `show_tags` BOOLEAN NOT NULL DEFAULT 0, `password` VARCHAR, `created_date` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, `modified_date` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, `footer_text` TEXT, `custom_css` TEXT, `show_powered_by` BOOLEAN NOT NULL DEFAULT 1, `analytics_id` VARCHAR, `show_certificate_expiry` BOOLEAN NOT NULL DEFAULT 0, `auto_refresh_interval` integer DEFAULT '300', `analytics_script_url` varchar(255), `analytics_type` varchar(255) CHECK (`analytics_type` in('google' , 'umami' , 'plausible' , 'matomo')) DEFAULT null, `show_only_last_heartbeat` boolean NOT NULL DEFAULT '0', `rss_title` varchar(255));
CREATE TABLE "stat_daily" (`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL, `monitor_id` integer NOT NULL, `timestamp` integer NOT NULL, `ping` float NOT NULL, `up` integer NOT NULL, `down` integer NOT NULL, `ping_min` float NOT NULL DEFAULT '0', `ping_max` float NOT NULL DEFAULT '0', `extras` text DEFAULT null, FOREIGN KEY (`monitor_id`) REFERENCES `monitor` (`id`) ON DELETE CASCADE ON UPDATE CASCADE);
CREATE INDEX [monitor_notification_index]
ON [monitor_notification](
  [monitor_id], 
  [notification_id]);
CREATE INDEX monitor_tag_monitor_id_index ON monitor_tag (monitor_id);
CREATE INDEX monitor_tag_tag_id_index ON monitor_tag (tag_id);
CREATE INDEX [fk]
    ON [monitor_group] (
                        [monitor_id],
                        [group_id]);
CREATE INDEX [good_index] ON [notification_sent_history] (
    [type],
    [monitor_id],
    [days]
);
CREATE INDEX [manual_active] ON [maintenance] (
    [strategy],
    [active]
);
CREATE INDEX [active] ON [maintenance] ([active]);
CREATE INDEX [maintenance_user_id] ON [maintenance] ([user_id]);
CREATE INDEX [status_page_id_index]
    ON [maintenance_status_page]([status_page_id]);
CREATE INDEX [maintenance_id_index]
    ON [maintenance_status_page]([maintenance_id]);
CREATE INDEX [maintenance_id_index2] ON [monitor_maintenance]([maintenance_id]);
CREATE INDEX [monitor_id_index] ON [monitor_maintenance]([monitor_id]);
CREATE INDEX proxy_user_id ON proxy (user_id);
CREATE UNIQUE INDEX `domain_expiry_domain_unique` on `domain_expiry` (`domain`);
CREATE INDEX [monitor_id] ON [heartbeat]([monitor_id]);
CREATE INDEX [important] ON [heartbeat]([important]);
CREATE INDEX monitor_time_index ON heartbeat (monitor_id, time);
CREATE INDEX `monitor_important_time_index` on `heartbeat` (`monitor_id`, `time`) where important = 1;
CREATE INDEX `heartbeat_important_index` on `heartbeat` (`important`) where important = 1;
CREATE UNIQUE INDEX `stat_minutely_monitor_id_timestamp_unique` on `stat_minutely` (`monitor_id`, `timestamp`);
CREATE UNIQUE INDEX `stat_hourly_monitor_id_timestamp_unique` on `stat_hourly` (`monitor_id`, `timestamp`);
CREATE INDEX user_id on monitor (user_id);
CREATE INDEX proxy_id ON monitor (proxy_id);
CREATE INDEX `monitor_remote_browser_index` on `monitor` (`remote_browser`);
CREATE UNIQUE INDEX [slug] ON [status_page]([slug]);
CREATE UNIQUE INDEX `stat_daily_monitor_id_timestamp_unique` on `stat_daily` (`monitor_id`, `timestamp`);

-- knex migrations applied, in filename order (`knex_migrations.name`):
-- migration: 2023-08-16-0000-create-uptime.js
-- migration: 2023-08-18-0301-heartbeat.js
-- migration: 2023-09-29-0000-heartbeat-retires.js
-- migration: 2023-10-08-0000-mqtt-query.js
-- migration: 2023-10-11-1915-push-token-to-32.js
-- migration: 2023-10-16-0000-create-remote-browsers.js
-- migration: 2023-12-20-0000-alter-status-page.js
-- migration: 2023-12-21-0000-stat-ping-min-max.js
-- migration: 2023-12-22-0000-hourly-uptime.js
-- migration: 2024-01-22-0000-stats-extras.js
-- migration: 2024-04-26-0000-snmp-monitor.js
-- migration: 2024-08-24-000-add-cache-bust.js
-- migration: 2024-08-24-0000-conditions.js
-- migration: 2024-10-1315-rabbitmq-monitor.js
-- migration: 2024-10-31-0000-fix-snmp-monitor.js
-- migration: 2024-11-27-1927-fix-info-json-data-type.js
-- migration: 2025-01-01-0000-add-smtp.js
-- migration: 2025-02-15-2312-add-wstest.js
-- migration: 2025-02-17-2142-generalize-analytics.js
-- migration: 2025-03-04-0000-ping-advanced-options.js
-- migration: 2025-03-25-0127-fix-5721.js
-- migration: 2025-05-09-0000-add-custom-url.js
-- migration: 2025-06-03-0000-add-ip-family.js
-- migration: 2025-06-11-0000-add-manual-monitor.js
-- migration: 2025-06-13-0000-maintenance-add-last-start.js
-- migration: 2025-06-15-0001-manual-monitor-fix.js
-- migration: 2025-06-24-0000-add-audience-to-oauth.js
-- migration: 2025-07-17-0000-mqtt-websocket-path.js
-- migration: 2025-09-02-0000-add-domain-expiry.js
-- migration: 2025-10-14-0000-add-ip-family-fix.js
-- migration: 2025-10-15-0000-stat-table-fix.js
-- migration: 2025-10-15-0001-add-monitor-response-config.js
-- migration: 2025-10-15-0002-add-response-to-heartbeat.js
-- migration: 2025-10-24-0000-show-only-last-heartbeat.js
-- migration: 2025-12-09-0000-add-system-service-monitor.js
-- migration: 2025-12-17-0000-add-globalping-monitor.js
-- migration: 2025-12-22-0121-optimize-important-indexes.js
-- migration: 2025-12-29-0000-remove-line-notify.js
-- migration: 2025-12-31-2143-add-snmp-v3-username.js
-- migration: 2026-01-02-0551-dns-last-result-to-text.js
-- migration: 2026-01-02-0713-gamedig-v4-to-v5.js
-- migration: 2026-01-05-0000-add-rss-title.js
-- migration: 2026-01-05-0000-add-tls-monitor.js
-- migration: 2026-01-06-0000-fix-domain-expiry-column-type.js
-- migration: 2026-01-10-0000-convert-float-precision.js
-- migration: 2026-01-15-0000-add-json-query-retry-only-status-code.js
-- migration: 2026-01-16-0000-add-screenshot-delay.js
-- migration: 2026-02-07-0000-disable-domain-expiry-unsupported-tlds.js
-- migration: 2026-03-27-0000-add-ntp-monitor.js
-- migration: 2026-05-20-0000-add-bearer-token.js
-- migration: 2026-05-25-0000-add-gamedig-token.js
-- migration: 2026-06-19-1411-add-rybbit-analytics-type.js
-- migration: 2026-06-19-1412-analytics-type-to-string.js
-- migration: 2026-07-22-0000-fix-stat-daily-overflow.js
