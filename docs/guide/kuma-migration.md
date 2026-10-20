# Migrate from Uptime Kuma

`v1per import kuma` moves an Uptime Kuma 2.5.0 installation into one node of V1Per: configuration, users, status pages and history. Push URLs keep their tokens, and imported uptime numbers match Kuma's.

The source is Kuma's SQLite database, `kuma.db`. Other Kuma versions are refused with a message that says what to do instead. The import reads `kuma.db` and never writes to it.

## Before you start

- Stop `v1per serve`. The import needs the data directory to itself and refuses with `server_running` otherwise. While an import is unfinished, `serve` refuses to start with `import_in_progress`.
- The target node must exist, be empty and have no child nodes. A node takes one import, ever. Importing into the root of a fresh instance is the simple case.
- Stop Kuma, or copy `kuma.db` together with its `kuma.db-wal` and `kuma.db-shm` files. After a clean stop Kuma has folded them into `kuma.db`.

## Run the import

Look first, then import:

```sh
v1per import kuma --data-dir /var/lib/v1per --source /backup/kuma.db --into Root --dry-run
v1per import kuma --data-dir /var/lib/v1per --source /backup/kuma.db --into Root
```

With Docker, stop the server container, mount the Kuma file and run the command on the same volume:

```sh
docker stop v1per
docker run --rm -v v1per-data:/data -v /backup:/backup:ro ghcr.io/coderscantina/v1per:latest \
  import kuma --source /backup/kuma.db --into Root --dry-run
```

`--into` takes the node's ID or its path from the root, such as `Root/Clients/Acme`. `V1PER_KUMA_DB` can stand in for `--source`.

| Option | Meaning |
| --- | --- |
| `--dry-run` | Prints counts per table, monitors by type and support, channels by provider, secrets to seal, heartbeats inside and outside the raw window, the inventory, the logins, and whether the target node can take the import. Writes nothing |
| `--login-domain` | Kuma usernames that are not email addresses sign in as `<username>@<domain>`. Default `kuma.invalid` |
| `--raw-days` | Days of raw heartbeats to import, default 90 |
| `--chunk-rows` | Source rows per chunk, default 5000. Lower it to use less memory |

The import runs in steps: configuration in one transaction, then rollups, then raw heartbeats, then a check against the source. If it stops (Ctrl-C, a crash, a full disk), run the same command again. It resumes where it stopped and stores nothing twice. A resume refuses a source file that changed since the start (`source_changed`) and options that differ (`options_changed`).

At the end it prints a summary and writes a manifest to `import-node-<id>.json` in the data directory. The manifest compares every Kuma table with what V1Per created, the history row counts, and the 24 hour, 30 day and 1 year uptime of every monitor with Kuma's own numbers. It also lists the logins. The command exits 1 with `not_reconciled` if anything differs.

Instance admins can read the import and its inventory in the admin app afterwards.

## What gets imported

| Kuma | V1Per |
| --- | --- |
| Monitors | Monitors on the target node. http, keyword, json-query, port, ping, dns, push, group and manual monitors run. Each starts from Kuma's last state, so a monitor that was already down does not alert again |
| Group monitors | Child nodes holding their members, plus a group monitor on that node |
| Notifications | Channels on the target node, with their monitor links and default flags |
| Maintenance and tags | Their V1Per counterparts |
| Status pages | Pages with their groups, components, domains, theme and language. Page passwords keep working |
| Incidents | An incident with one update, resolved unless the banner was still shown |
| Users | Accounts with an owner role on the target node. The first Kuma admin also becomes instance admin |
| `stat_*` rollups | All of them, kept as they are |
| Heartbeats | The last `--raw-days` days |

Older history comes from Kuma's rollups, so uptime numbers stay complete. Kuma may hold less raw history than you expect if its "keep data" setting was short.

What does not come over, or comes over inactive:

- Monitors V1Per cannot run (other Kuma types, NTLM, mTLS, Kuma proxies, conditions) are imported paused with the reason.
- Channels of providers V1Per cannot send to are imported inactive with the reason. You can delete them.
- Custom CSS, analytics scripts and status page logos are not imported. Docker hosts, remote browsers, API keys and settings are listed in the inventory with a reason.
- Sessions and API keys are not imported. Issue new API keys.

What changes for people:

- Kuma passwords keep working and are rehashed at the first sign-in. Authenticator apps keep working.
- Kuma usernames that are not email addresses become `<username>@<login-domain>`.
- Status pages move from `/status/<slug>` to `/s/<slug>`. A published page becomes public, a page with a password asks for it, an unpublished page is for people with a role on the node.

## Push URLs

V1Per answers `/api/push/<token>` like Kuma 2.5.0 does, and imported push monitors keep their tokens. A push client only needs the new host, or nothing if the host name moves to V1Per.

- Any HTTP method works. Parameters come from the query string: `status`, `msg` and `ping`.
- No `status`, an empty one or `status=up` is up. Anything else is down. `msg` defaults to `OK`.
- Success answers `200 {"ok":true}`. An unknown or paused monitor answers `404 {"ok":false,"msg":"Monitor not found or not active."}`.

Unlike Kuma, V1Per limits pushes: 120 a minute per token, and 30 unknown tokens a minute per client address. Past that it answers `429`. A push it could not record answers `503`, so the sender retries.

## Cutover checklist

Rehearse on a copy first, then move for real. Keep Kuma installed until you are sure.

1. **Rehearse.** Copy `kuma.db` with its `-wal` and `-shm` files. Run `v1per init` on a scratch data directory and import into it, first with `--dry-run`. Read the summary and the manifest. The scratch copy holds your real channels, so switch them off before you start `serve` on it, for example with `sqlite3 <scratch-dir>/control.sqlite "UPDATE channels SET active = 0"` while it is stopped.
2. **Prepare V1Per.** Install it, run `v1per init`, start it once, create the node to import into if it is not the root, then stop `v1per serve`.
3. **Stop Kuma**, so no beat arrives after the copy: `docker stop uptime-kuma` or the equivalent. Push clients now fail until step 6, and Kuma's checks pause.
4. **Copy `kuma.db`** from Kuma's data directory, with `kuma.db-wal` and `kuma.db-shm` if they exist.
5. **Import** with `--dry-run`, then for real. The summary must say `reconciled` and `source unchanged`.
6. **Start `v1per serve`.** Check that monitors report, that a channel's test message arrives, and that a status page opens at `/s/<slug>`.
7. **Switch DNS and push clients.** Point each status page domain and the push clients' host at V1Per, and add the page domains to your reverse proxy. If people use old `/status/<slug>` addresses, redirect them to `/s/<slug>` at the proxy.
8. **Tell your users** the new sign-in address, their login if they had a username, and that API keys need reissuing.
9. **Keep Kuma** stopped but installed, with its data. To roll back, stop V1Per, point DNS and push clients back and start Kuma. Kuma never saw the beats V1Per recorded in between.
