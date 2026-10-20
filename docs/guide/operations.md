# Operations

How to back up, restore and upgrade V1Per, what it cleans up by itself, and how to check its health. The examples use a data directory at `/var/lib/v1per`. In Docker the data directory is `/data`, so you can leave out `--data-dir` there.

## Data directory

| File | Holds |
| --- | --- |
| `control.sqlite` | Configuration, accounts, monitors, incidents, pages and the notification queue |
| `inbox.sqlite` | New check results waiting to move into heartbeat storage |
| `heartbeats.duckdb` | Heartbeat history and uptime rollups |
| `secrets.key` | The secrets key, unless you keep it elsewhere. See [Secrets key](configuration.md#secrets-key) |
| `v1per.lock` | Held by the process that uses the directory |
| `v1per.initialized` | Written when `init` or `restore` finished |
| `import-node-<id>.json` | The manifest of a Kuma import |

`v1per.init-in-progress`, `v1per.restore-in-progress` and `v1per.upgrade-in-progress` mark an unfinished command. `serve` refuses a directory that has one. Run the same command again to finish it.

Never copy single files into or out of a running data directory. Use `backup` and `restore`.

`serve` stops admitting new results when free disk space on the data volume falls below 4 GiB. Watch free space with `diagnostics`.

## Health endpoint

`GET /health` answers `200` with the version once the server is up:

```json
{"status":"ok","version":"26.10.19"}
```

The image's healthcheck calls it every 30 seconds.

## Logs

The server logs to stderr, so `docker logs v1per` shows them. Set the level with `V1PER_LOG`, see [Logs](configuration.md#logs). At start it logs the address it listens on, and warns if `V1PER_PUBLIC_URL` is not set.

## Backup

```sh
v1per backup --data-dir /var/lib/v1per --out /mnt/backups/v1per-2026-10-19.tar
```

With Docker, mount a backup volume and run it in the container:

```sh
docker exec v1per v1per backup --out /backups/v1per-2026-10-19.tar
```

- The backup is one tar file: `manifest.json`, then `control.sqlite`, `inbox.sqlite` and `heartbeats.duckdb`. The manifest holds the schema versions, the secrets key's fingerprint and a checksum of every file. `tar -tf` lists it.
- You can back up while `serve` runs. Checks, push endpoints, probe agents and pages keep working, and the command prints how long heartbeat storage paused, for example `transfer paused 21 ms`.
- The command reads the archive back and checks every file before it reports success. It refuses an `--out` file that exists.
- Write backups to other storage than the data volume. While it runs, a backup needs about twice the size of `heartbeats.duckdb` free there.
- The archive is readable by its owner only. It holds every account and every encrypted secret, so keep it private.

### Backups and the secrets key

A backup holds the key's fingerprint, not the key. Keep the key apart from the backups, for example in a password manager. Without it a restore refuses to run, and nothing could read the stored secrets anyway.

`--include-key` puts the key into the archive. The command warns about it and the manifest records it. Anyone with such a file can read every stored secret, so keep it encrypted and apart from ordinary backups.

## Restore

Restore into a new, empty directory, then start the server on it:

```sh
v1per restore --from /mnt/backups/v1per-2026-10-19.tar --data-dir /var/lib/v1per-restored \
  --secrets-key-file /secure/secrets.key
v1per serve --data-dir /var/lib/v1per-restored
```

- Restore never overwrites a data directory.
- Pass the instance's key with `--secrets-key-file` or `V1PER_SECRETS_KEY`, unless the backup holds it. If the instance kept the key as `secrets.key` in its data directory, restore writes it there again. If it came from `V1PER_SECRETS_KEY` or a key file, configure `serve` the same way as before.
- Restore checks every file against the manifest before it marks the directory ready, and prints how many waiting results it moved into heartbeat storage.
- The restored server resumes checks, sends pending notifications and serves the status pages. Results a probe agent delivered after the backup are not in it.

| Refusal (exit 3) | What to do |
| --- | --- |
| `secrets_key_required` | The backup does not hold its key. Pass it. The message names the fingerprint it needs |
| `secrets_key_mismatch` | The given key is another one. Find the key with the fingerprint the message names |
| `directory_not_empty`, `already_initialized` | Restore into a new, empty directory |
| `backup_corrupt`, `backup_invalid` | The file is damaged or not a V1Per backup. Use another one |
| `data_newer_than_binary` | Install the release that took the backup, or a newer one |
| `restore_mismatch`, `restore_invalid` | Keep the directory for diagnosis and restore another backup |

A backup of an older release restores as `upgrade_required`. Run `v1per upgrade` on the restored directory before `serve`. An interrupted restore leaves `v1per.restore-in-progress`. Run the same restore again.

## Upgrade

1. Pull the new image or install the new release. Stop the server.
2. Back up and migrate:

   ```sh
   v1per upgrade --data-dir /var/lib/v1per --backup-to /mnt/backups/v1per-before-upgrade.tar
   ```

   With Docker, run the new image on the volume:

   ```sh
   docker stop v1per
   docker run --rm -v v1per-data:/data -v /mnt/backups:/backups ghcr.io/coderscantina/v1per:26.10.19 \
     upgrade --backup-to /backups/v1per-before-upgrade.tar
   ```

3. Start the server with the new release.

On data from an older release, `serve` exits 3 with `upgrade_required`. `upgrade` takes and checks a backup first, then migrates. On current data it says so and changes nothing.

If a step fails, the command exits 1 with `upgrade_failed` and prints both ways out: fix the cause and run the same command again, which resumes with the backup it already took, or restore that backup into a new directory and run the old release on it. Until one of them succeeds, `serve` exits 3 with `upgrade_interrupted`.

For unattended upgrades, set `V1PER_MIGRATE=true`. `serve` then applies migrations at start without taking a backup, so back up on your own schedule.

## Retention

`serve` cleans up every hour, the first time 10 minutes after it starts:

| What | Kept |
| --- | --- |
| Raw heartbeats | 90 days |
| Minute, hour and day rollups | kept, so uptime and bars stay complete |
| Finished notification deliveries, finished maintenance jobs | 30 days |
| Batch IDs from probe agents | 24 hours |
| Sessions, sign-in steps, status page sessions, sign-in links | until they expire. Sessions also end after 7 idle days |

Run it now, or see what it would remove:

```sh
v1per retention --data-dir /var/lib/v1per --dry-run
v1per retention --data-dir /var/lib/v1per
```

A run that fails or is interrupted is finished by the next one. DuckDB reuses freed space without shrinking its file.

## Diagnostics

```sh
v1per diagnostics --data-dir /var/lib/v1per
v1per diagnostics --data-dir /var/lib/v1per --json
```

It prints the versions, file sizes, free disk and the reserve, the inbox depth and age, every probe with its status, lag and buffer, the notification backlog and failures of the last day, the retention policy and last run, recent jobs and imports. `--json` prints the document instance admins get from the API query `instance.operations`. It holds no credentials, check messages or addresses, so it can go into a bug report.

## Commands beside a running server

While `serve` runs, `backup`, `retention` and `diagnostics` hand their work to it and wait for the result. Without a server they open the data directory themselves and need the secrets key like `serve` does. `restore`, `upgrade` and `import kuma` need the server stopped.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Internal error, or a failed import or upgrade |
| 2 | Wrong usage, such as a missing flag or a bad value |
| 3 | Refused, such as `upgrade_required` or `secrets_key_required`. The message says why |
| 4 | Storage unavailable, such as a missing or unwritable directory |

Errors print as `error: <code>: <message>` on stderr.
