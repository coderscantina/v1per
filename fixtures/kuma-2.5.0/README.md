# Kuma 2.5.0 fixture

A synthetic Uptime Kuma 2.5.0 SQLite database for import tests. The crate `v1per-kuma-fixture` (`crates/kuma-fixture`) generates it; the database itself is never committed.

| File | Content |
| --- | --- |
| `schema.sql` | `.schema` of a Kuma 2.5.0 database plus the 54 knex migration names. Schema only. |
| `manifest.json` | Source pins, the documented credentials and the expected row counts of a generated file. |
| `.gitattributes` | Keeps `schema.sql` byte for byte; four statements carry CRLF line endings from Kuma's original template, and SQLite stores them that way. |

## Generate

```sh
cargo run -p v1per-kuma-fixture -- target/kuma-2.5.0.db
cargo run -p v1per-kuma-fixture -- --force --manifest fixtures/kuma-2.5.0/manifest.json target/kuma-2.5.0.db
```

Tests call `v1per_kuma_fixture::generate(path)` and open the result with `open_immutable(path)`. A debug build takes about 5 s; the file is about 27 MB.

`cargo test -p v1per-kuma-fixture` checks that two runs write byte-identical files, that the counts match `manifest.json`, that `sqlite_master` equals `schema.sql` statement for statement, that `knex_migrations` lists the 54 migrations, and that the documented credentials verify against the stored hashes.

Determinism holds for one build: same seed (`0x56317065725f6b75`), same fixed clock (2026-10-01 00:00:00 UTC) and the same bundled SQLite. Updating SQLite or the dependencies can change bytes but not counts; when a change to the generator changes counts, regenerate `manifest.json` with the second command above.

## Schema

`schema.sql` was dumped from a production Kuma 2.5.0 database (template database, legacy SQL patches, then all 54 knex migrations). A fresh install takes the same path. Checked against the source:

- every table and column the 54 migrations create is present, and the migration names equal `db/knex_migrations/*.js` in filename order;
- the dumped database applied the migrations in five batches, out of filename order, so `monitor` column order can differ from a fresh install. Importers must match columns by name;
- `status_page.analytics_type` keeps its `CHECK (google, umami, plausible, matomo)` constraint even after `2026-06-19-1412-analytics-type-to-string`, so a Kuma SQLite database cannot hold `rybbit`;
- `sqlite_sequence` and `sqlite_stat1` are SQLite internal tables. The generator runs `ANALYZE`, as Kuma's retention job runs `PRAGMA optimize`, and drops the `sqlite_stat4` table the bundled SQLite adds.

The file format matches Kuma: 4096 byte pages, `auto_vacuum = INCREMENTAL`, WAL journal, UTF-8. The decision 0007 evidence counts 55 migrations; the 2.5.0 source and the real database both have 54.

## Contents

All hostnames are `example.com`, `example.org` or `example.net`, addresses come from the documentation ranges, and every secret is an obvious fixture value.

- **Monitors**: 49 covering all 33 Kuma 2.5.0 types, with their type-specific columns: http (basic, OAuth client credentials, mTLS, bearer, NTLM, upside down, paused), keyword (normal and inverted), json-query, port, ping, dns, docker (socket and TCP host), system-service, pm2, real-browser, group, push, manual (set and unset), globalping, grpc-keyword, kafka-producer, mqtt, ntp, rabbitmq, sip-options, smtp, snmp, tailscale-ping, websocket-upgrade, sqlserver, mongodb, mysql, oracledb, postgres, radius, redis, gamedig, steam.
- **Groups**: Production > EU > Web > Edge (four levels), Staging, and an empty group.
- **Notifications**: 13 channels. All eight core providers (two smtp, two discord, webhook, slack, teams, telegram, ntfy, pushover) plus gotify, matrix and mattermost. One email channel has a custom Liquid subject and HTML body, one Discord channel a custom Liquid template; the other Discord channel posts to a thread with a prefix message. One email channel is the default.
- **Maintenance**: 7 windows covering all six strategies (manual, two single, recurring-interval, recurring-weekday, recurring-day-of-month, cron), linked to monitors, a group and the public status page.
- **Status pages**: `status` (published, custom CSS, footer, Plausible analytics, two custom domains, pinned incident), `client-acme` (password, dark theme, custom CSS, one domain, unpinned incident) and `internal` (unpublished, Umami analytics, last heartbeat only), with five groups.
- **Other**: 4 tags with values, 2 proxies (HTTP with auth, SOCKS5), a remote browser, 2 docker hosts, TLS info for two monitors with certificate reminders sent for one, 2 domain expiry rows, 17 settings including `keepDataPeriodDays`, 2 API keys.

## History

Beats follow Kuma's check loop and push router, including retries, upside down, `important` flags, resend counts and maintenance. Rollups replay those beats through a copy of Kuma's uptime calculator, so `stat_*` rows agree with the heartbeats the way Kuma's do.

| Monitor | Type | Interval | History |
| --- | --- | --- | --- |
| API | http | 300 s | 400 days, random outages, weekly and single maintenance windows, saved 503 response bodies |
| Shop keyword | keyword | 600 s | 400 days, random outages, day-of-month maintenance |
| Nightly backup | push | 3600 s | 400 days of hourly pushes with missed runs ("No heartbeat in the time window") and failed runs (`status=down`) |
| Gateway ping | ping | 60 s | 2 days, cron maintenance in Asia/Kolkata, latency as REAL |
| Oracle, Staging | oracledb, group | 300 s, 60 s | 6 hours, down for the last 70 minutes, the group failing with it |
| Manual unset, Empty group | manual, group | 60 s | 2 hours of pending beats |
| Other active monitors | | own interval | 6 hours, up |

Retention follows Kuma: `stat_minutely` holds the last 24 hours, `stat_hourly` the last 30 days, `stat_daily` all 400 days. `keepDataPeriodDays` is 365, so the oldest 35 days of heartbeats and day rows lie outside the retention window, as if the setting was lowered after the last cleanup. Maintenance counts are in `extras` as `{"maintenance":n}`.

## Credentials

| What | Value | Stored as |
| --- | --- | --- |
| User | `fixture-admin` / `fixture-admin-password` | bcrypt `$2a$10$` |
| TOTP | `FixtureTotpSecretNotRealFixtureTotpSecretNotRealFixtureTotp00001` | plain `twofa_secret`; Kuma uses its ASCII bytes as the HMAC key and shows base32 of those bytes without padding in the `otpauth://` URI |
| API keys | `uk1_fixtureApiKeyNotReal0000000000000000001` (active), `uk2_fixtureApiKeyNotReal0000000000000000002` (inactive, expired) | bcrypt of the part after `uk<id>_` |
| Page `client-acme` | `fixture-page-password` | plain text |

## Kuma storage details

Formats an importer needs, all reproduced by the generator:

- Heartbeat `time` and `end_time` are UTC text `YYYY-MM-DD HH:MM:SS.mmm`. `ping` is an integer, REAL for the ping type.
- `heartbeat.response` is base64 of a brotli-compressed body.
- Settings values are JSON encoded, so strings keep their quotes. `jwtSecret` is the exception: a raw bcrypt hash.
- `notification.config` is the whole form as JSON, including `name`, `type`, `isDefault` and `applyExisting`.
- `knex_migrations.migration_time` is an integer in milliseconds.
- `stat_*.timestamp` is unix seconds at the start of the minute, hour or UTC day.
