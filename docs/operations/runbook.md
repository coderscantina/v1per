# Runbook

How to install, run, back up, restore and upgrade v1per. Key rotation is not built yet.

## What you run

One process, `v1per serve`, with a data directory. It needs no Node, database server or queue. The same binary runs remote probes with `v1per probe`.

The data directory holds the SQLite and DuckDB files and `secrets.key`. Keep the key with every copy of the data. Without it the stored secrets cannot be read.

## Install with Docker

The image `ghcr.io/coderscantina/v1per` runs as a non-root user, keeps its data in the `/data` volume and listens on port 8080.

Create the data directory once. This makes the instance admin and the secrets key, and prints the admin password a single time:

```sh
docker run --rm -v v1per-data:/data ghcr.io/coderscantina/v1per:latest \
  init --admin-email you@example.com
```

Pass `--admin-password-file` to choose the password instead. Then start the server:

```sh
docker run -d --name v1per --restart unless-stopped --stop-timeout 35 \
  -v v1per-data:/data -p 127.0.0.1:8080:8080 \
  -e V1PER_PUBLIC_URL=https://status.example.com \
  ghcr.io/coderscantina/v1per:latest
```

`/health` answers once the server is up. The image has a healthcheck on it.

[deploy/docker-compose.yml](../../deploy/docker-compose.yml) does the same with Compose and has an optional probe service. [deploy/coolify/docker-compose.yml](../../deploy/coolify/docker-compose.yml) is the Coolify variant: set the service domain to the admin host with the container port, for example `https://status.example.com:8080`.

### Settings

Each flag has an environment variable. See `v1per serve --help` for the full list.

| Variable | Meaning |
| --- | --- |
| `V1PER_DATA_DIR` | Data directory. `/data` in the image |
| `V1PER_BIND` | Listen address. `0.0.0.0:8080` in the image, `127.0.0.1:8080` otherwise |
| `V1PER_PUBLIC_URL` | URL of the admin app. Commands must come from this origin, and `https` marks the session cookie `Secure`. Set it behind a proxy |
| `V1PER_INSTANCE_NAME` | Name shown in the admin app |
| `V1PER_WEB_DIR` | Web app directory. Default: `web` next to the executable |
| `V1PER_SECRETS_KEY_FILE` | Secrets key file, if it should live outside the data directory |
| `V1PER_WORKER_THREADS` | Tokio worker threads, default 2 |
| `V1PER_MIGRATE` | `true` applies schema migrations at start without a backup, see [Upgrade](#upgrade) |

## Install the binary

Build the package on a machine with Rust and Bun:

```sh
apps/server/scripts/package.sh [output-dir]
```

It writes `target/package/v1per/` with the `v1per` binary and the `web/` folder beside it. Copy the folder to the server, then:

```sh
./v1per init --data-dir /var/lib/v1per --admin-email you@example.com
V1PER_PUBLIC_URL=https://status.example.com ./v1per serve --data-dir /var/lib/v1per
```

Run it under systemd or similar, as an unprivileged user, and give it 30 seconds or more to stop on SIGTERM.

## Reverse proxy and custom domains

Run TLS at a reverse proxy. v1per serves plain HTTP and picks a status page by the `Host` header, so the proxy must pass the original Host through and must not rewrite it.

[deploy/Caddyfile](../../deploy/Caddyfile) is an example. Caddy sends the Host header unchanged and gets a certificate for each listed domain:

```
status.example.com {
	reverse_proxy 127.0.0.1:8080
}

status.customer.example {
	reverse_proxy 127.0.0.1:8080
}
```

To add a custom domain for a status page:

1. Claim the hostname on the status page in the admin app.
2. Point the domain's DNS record at the proxy.
3. Add the domain to the proxy so it requests a certificate.

Nginx needs `proxy_set_header Host $host;`. Traefik and Coolify pass it by default. Server-sent events carry the live updates, so do not buffer responses: Caddy does this by default, nginx needs `proxy_buffering off;`.

## Deploy a probe

A probe checks monitors from another place and only connects out to the server, over HTTPS. It needs no open port.

1. In the admin app, register a probe on a node. The token is shown once.
2. Start the agent on the remote host. Prefer the environment variable for the token, since a flag shows in the process list:

```sh
docker run -d --name v1per-probe --restart unless-stopped --no-healthcheck \
  -v v1per-probe-data:/probe-data \
  -e V1PER_PROBE_SERVER=https://status.example.com \
  -e V1PER_PROBE_TOKEN=... \
  ghcr.io/coderscantina/v1per:latest probe
```

The image's healthcheck asks the server port, so a probe container turns it off with `--no-healthcheck`.

Or with the binary: `V1PER_PROBE_TOKEN=... v1per probe --server https://status.example.com --data-dir /var/lib/v1per-probe`.

The data directory buffers results while the server is unreachable, up to `--buffer-mb` (default 64). Only one agent at a time may use it.

### ICMP

Ping monitors need ICMP. The agent uses unprivileged ping sockets, which work when the group id of the process is inside `net.ipv4.ping_group_range`. Docker sets this range for containers, so the container needs nothing extra. On a binary install, check the range and widen it if needed:

```sh
sysctl net.ipv4.ping_group_range
sudo sysctl -w net.ipv4.ping_group_range="0 2147483647"
```

If that is not possible, give the binary raw sockets instead: `--cap-add NET_RAW` for Docker or `setcap cap_net_raw+ep v1per`.

## Import from Uptime Kuma

`v1per import kuma` brings an Uptime Kuma 2.5.0 SQLite database into one node: monitors, groups as child nodes, notification channels, maintenance, status pages with their domains and passwords, incidents, users with their passwords and authenticators, all `stat_*` rollups and 90 days of raw heartbeats. Push URLs keep their tokens. Other Kuma versions are refused with what to do instead.

The import reads `kuma.db` and never writes it. It needs the data directory to itself, so stop `v1per serve` first; while an import is unfinished, `serve` refuses to start (`import_in_progress`). The target node must be new and empty, and a node takes one import, ever.

```sh
v1per import kuma --data-dir /var/lib/v1per --source /backup/kuma.db --into Root --dry-run
v1per import kuma --data-dir /var/lib/v1per --source /backup/kuma.db --into Root
```

`--into` takes the node's ID or its path from the root, such as `Root/Clients/Acme`. `V1PER_KUMA_DB` can stand in for `--source`.

| Option | Meaning |
| --- | --- |
| `--dry-run` | Prints counts per table, monitors by type and support, channels by provider, secrets to seal, heartbeats inside and outside the raw window, the inventory, the logins, and whether the target node can take the import. Writes nothing |
| `--login-domain` | Kuma usernames that are not email addresses sign in as `<username>@<domain>`. Default `kuma.invalid` |
| `--raw-days` | Days of raw heartbeats to import, default 90. Older history comes from Kuma's rollups, so uptime numbers stay complete |
| `--chunk-rows` | Source rows per chunk, default 5000 |

The import runs in steps: configuration in one transaction, then rollups, then raw heartbeats, then a check against the source. If it stops (Ctrl-C, a crash, a full disk), run the same command again: it resumes where it stopped and stores nothing twice. A resume refuses a source file that changed since the start (`source_changed`) and options that differ (`options_changed`).

At the end it prints a summary and writes the manifest to `import-node-<id>.json` in the data directory. The manifest compares every Kuma table with what v1per created or inventoried, the history row counts, and the 24 hour, 30 day and 1 year uptime of every monitor with Kuma's own numbers. It also lists the logins. The command exits 1 with `not_reconciled` if anything differs.

What the import changes for people:

- Kuma users get an owner grant on the target node; the first Kuma admin also becomes instance admin. Their password works and is rehashed at the first sign-in. Their authenticator keeps working. Sessions and API keys are not imported: issue new API keys.
- Monitors v1per cannot run (other Kuma types, NTLM, mTLS, Kuma proxies, conditions) are imported paused with the reason. Channels of providers v1per cannot send to are imported inactive.
- Status pages move from `/status/<slug>` to `/s/<slug>`. Custom CSS, analytics scripts and logos are not imported. A published page becomes public, a page with a password asks for it, an unpublished page is for grant holders.

Instance admins can read the import and its inventory through `imports.get` and `imports.inventory`.

## Cut over from Uptime Kuma

Rehearse on a copy first, then move for real. Kuma stays installed until you are sure.

1. Rehearse: copy `kuma.db` with its `-wal` and `-shm` files, run the import with `--dry-run`, then for real into a scratch data directory. Read the summary and the manifest. Switch off the scratch copy's channels before you start `serve` on it, or it alerts your real channels.
2. Prepare v1per: install it, run `v1per init`, start it once, create the node to import into if it is not the root, then stop `v1per serve`.
3. Stop Kuma, so no beat arrives after the copy: `docker stop uptime-kuma` or the equivalent. Push clients now fail until step 6; Kuma's own checks pause.
4. Copy `kuma.db` from Kuma's data directory. Copy `kuma.db-wal` and `kuma.db-shm` with it if they exist; after a clean stop Kuma has folded them into `kuma.db`.
5. Import with `--dry-run`, then for real. The summary must say `reconciled` and `source unchanged`.
6. Start `v1per serve`. Check that monitors report, that a channel's test message arrives, and that a status page opens at `/s/<slug>`.
7. Switch traffic: point each status page domain and the push clients' host at v1per. Push URLs keep their path and token, so a client needs only the new host, or nothing if the host name moves to v1per. Link old `/status/<slug>` addresses to `/s/<slug>` at the proxy if people use them.
8. Tell users: new sign-in address, logins under `--login-domain`, API keys to reissue.
9. Keep Kuma stopped but installed, with its data, for rollback. To roll back, stop v1per, point DNS and push clients back and start Kuma. Kuma never saw the beats v1per recorded in between.

## Backup

```sh
v1per backup --data-dir /var/lib/v1per --out /mnt/backups/v1per-2026-10-18.tar
```

With Docker, write to a mounted backup volume: `docker exec v1per v1per backup --out /backups/v1per-2026-10-18.tar`.

- The backup is one tar file: `manifest.json`, then `control.sqlite`, `inbox.sqlite` and `heartbeats.duckdb`. The manifest holds the schema versions, the ledger positions, the secrets key's fingerprint and a BLAKE3 checksum of every file. `tar -tf` lists it.
- While `serve` runs, the server takes the backup. Heartbeat transfer pauses while DuckDB and the inbox are copied; checks, push endpoints, probe agents and pages go on, and new results wait in the inbox. The command prints how long the pause was (`transfer paused 21 ms`).
- With the server stopped, the command opens the data directory itself.
- The command reads the archive back and checks every file before it reports success. It writes `.<name>.partial` and `.<name>.staging` next to the archive while it works and removes them when it fails. It refuses an `--out` file that exists.
- Write backups to other storage than the data volume. While it runs, a backup needs about twice the size of `heartbeats.duckdb` free on the backup volume.
- The archive is readable by its owner only. It holds every account and sealed secret, so keep it private.

### The secrets key

A backup holds a reference to the key, not the key: its fingerprint. Keep the key apart from the backups, for example in a password manager. Without it a restore refuses to run, and nothing could open the stored secrets anyway.

`--include-key` puts the key into the archive. The command warns about it on stderr and the manifest records it. Anyone who has such a file can read every stored secret: keep it encrypted and apart from ordinary backups.

## Restore

```sh
v1per restore --from /mnt/backups/v1per-2026-10-18.tar --data-dir /var/lib/v1per-restored \
  --secrets-key-file /secure/secrets.key
v1per serve --data-dir /var/lib/v1per-restored
```

- The target directory must be missing or empty. Restore never overwrites a data directory.
- Pass the key the instance used with `--secrets-key-file` or `V1PER_SECRETS_KEY`, unless the backup holds it. If the instance kept it as `secrets.key` in its data directory, restore writes it there again. If it came from `V1PER_SECRETS_KEY` or a key file, configure `serve` the same way as before.
- Restore reads the manifest first, then checks every file against its checksum, opens everything once, moves the results that waited in the backup's inbox into DuckDB and checks the ledger against the manifest before it marks the directory initialized. It prints how many waiting records it moved.
- The restored server resumes checks, sends the notifications that were pending and serves the status pages. Results a probe agent delivered after the backup are not in it; nothing is stored twice.

| Refusal (exit 3) | What to do |
| --- | --- |
| `secrets_key_required` | The backup does not hold its key. Pass it; the message names the fingerprint it needs |
| `secrets_key_mismatch` | The given key is another one. Find the key with the fingerprint the message names |
| `directory_not_empty`, `already_initialized` | Restore into a new, empty directory |
| `backup_corrupt`, `backup_invalid` | The file is damaged or not a v1per backup. Use another one. The target directory is left empty |
| `data_newer_than_binary` | Install the release that took the backup, or a newer one |
| `restore_mismatch`, `restore_invalid` | The restored engines do not add up. Keep the directory for diagnosis and restore another backup |

A backup of an older release restores with state `upgrade_required`: run `v1per upgrade` on the restored directory before `serve`. An interrupted restore leaves `v1per.restore-in-progress`; `serve` refuses the directory. Run the same restore again; it starts over.

Never copy single engine files into a data directory. Restore a whole backup.

## Upgrade

1. Install the new release. Stop the server.
2. `v1per upgrade --data-dir /var/lib/v1per --backup-to /mnt/backups/v1per-before-upgrade.tar`
3. Start the server.

On data from an older release `serve` exits 3 with `upgrade_required`. The upgrade takes and verifies a backup first, then migrates `control.sqlite`, then `inbox.sqlite`. On current data it says so and changes nothing.

If a step fails, the command exits 1 with `upgrade_failed` and prints both ways out: fix the cause and run the same command again, which resumes with the backup it already took, or restore that backup into a new directory and run the old release on it. Until one of them succeeds, `serve` exits 3 with `upgrade_interrupted`.

`serve --migrate` (or `V1PER_MIGRATE=true`) applies migrations at start without a backup, for deployments that upgrade unattended and back up on their own schedule.

## Key rotation

Not built. The key in use stays the key of every backup taken with it. To replace a key that leaked, there is no command yet: every sealed secret (monitor credentials, notification settings, SMTP password, second factors) would have to be entered again.

## Retention

`serve` runs retention every hour, the first time 10 minutes after it starts:

| What | Kept |
| --- | --- |
| Raw heartbeats | 90 days |
| Minute rollups | forever, until Mike decides on [0002](../decisions/0002-storage-split-and-retention.md); hour and day rollups always |
| Probe batch IDs, used to recognize a resent batch | 24 hours |
| Finished notification deliveries, finished maintenance jobs | 30 days |
| Sessions, login steps, status page sessions, sign-in links | until they expire; sessions also end after 7 idle days |

Run it now, or see what it would remove:

```sh
v1per retention --data-dir /var/lib/v1per --dry-run
v1per retention --data-dir /var/lib/v1per
```

A run that fails, for example on a full disk, or that a stop interrupts, is finished by the next one: every step skips what is already gone. DuckDB reuses freed space without shrinking its file.

## Diagnostics

```sh
v1per diagnostics --data-dir /var/lib/v1per
v1per diagnostics --data-dir /var/lib/v1per --json
```

It prints the versions, engine file sizes, free disk and the reserve below which new results are refused, inbox depth and age, the DuckDB watermark and transfer failures, every probe with its status, lag and buffer, the outbox backlog and failures of the last day, the retention policy and last run, recent jobs and imports. `--json` prints the document instance admins get from `instance.operations`. It holds no credentials, check messages or addresses, so it can go into a bug report.

While a server runs, `backup`, `retention` and `diagnostics` hand their work to it and wait. Without one they take the data directory themselves and need the secrets key like `serve` does.
