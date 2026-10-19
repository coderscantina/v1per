# Configuration

v1per is configured with command-line flags. Most flags have a `V1PER_*` environment variable, which is the easier way in Docker. A flag wins over its variable. `v1per <command> --help` prints the same list.

Settings you change at runtime, such as the instance name, timezone, mail server and certificate warning days, live in the admin app under the instance settings.

## Commands

| Command | What it does |
| --- | --- |
| `v1per init` | Creates a data directory with the instance admin and the secrets key |
| `v1per serve` | Runs the server: admin app, API, status pages, push endpoint, probe endpoints and checks. The image's default |
| `v1per import kuma` | Imports an Uptime Kuma 2.5.0 database. See [Migrate from Uptime Kuma](kuma-migration.md) |
| `v1per backup` | Writes a verified backup archive |
| `v1per restore` | Restores a backup into a new, empty data directory |
| `v1per upgrade` | Backs up a stopped data directory, then migrates it to this release |
| `v1per retention` | Removes expired heartbeats and records now |
| `v1per diagnostics` | Prints the health of storage, probes, notifications, retention and imports |

The last five are covered in [Operations](operations.md).

The remote probe agent is a separate program, `v1per-probe`. See [Probes](probes.md#run-an-agent).

## Environment variables

| Variable | Flag | Used by | Default | Meaning |
| --- | --- | --- | --- | --- |
| `V1PER_DATA_DIR` | `--data-dir` | all `v1per` commands | `/data` in the image; `data` for `serve` and `import kuma` outside it | The data directory. `init`, `backup`, `restore`, `retention`, `diagnostics` and `upgrade` require it |
| `V1PER_BIND` | `--bind` | `serve` | `0.0.0.0:8080` in the image, `127.0.0.1:8080` otherwise | Address and port to listen on |
| `V1PER_PUBLIC_URL` | `--public-url` | `serve` | none | The URL the admin app is reached at, see [Public URL](#public-url) |
| `V1PER_INSTANCE_NAME` | `--instance-name` | `serve` | the name set in the admin app, else `v1per` | Name shown in the admin app and the authenticator app |
| `V1PER_WEB_DIR` | `--web-dir` | `serve` | `web` next to the executable | The built admin app. The image sets it up for you |
| `V1PER_WORKER_THREADS` | `--worker-threads` | `serve` | `2` | Worker threads of the server's runtime. Raise it on hosts with more cores |
| `V1PER_MIGRATE` | `--migrate` | `serve` | off | Applies schema migrations at start without a backup, see [Upgrade](operations.md#upgrade) |
| `V1PER_SECRETS_KEY` | none | all `v1per` commands | none | The secrets key itself, base64 of 32 bytes. Environment only, see [Secrets key](#secrets-key) |
| `V1PER_SECRETS_KEY_FILE` | `--secrets-key-file` | all `v1per` commands | `secrets.key` in the data directory | A file holding the secrets key |
| `V1PER_KUMA_DB` | `--source` | `import kuma` | none | Path of Kuma's `kuma.db` |
| `V1PER_PROBE_SERVER` | `--server` | `v1per-probe` | none, required | The v1per server URL, for example `https://status.example.com` |
| `V1PER_PROBE_TOKEN` | `--token` | `v1per-probe` | none, required | The token shown when the probe was registered |
| `V1PER_PROBE_DATA_DIR` | `--data-dir` | `v1per-probe` | `/probe-data` in the image, `probe-data` otherwise | Where the agent buffers results |
| `V1PER_PROBE_BUFFER_MB` | `--buffer-mb` | `v1per-probe` | `64` | Largest result buffer on disk, in MiB |
| `V1PER_LOG` | none | all | `info,sqlx=warn`; `info` for `v1per-probe` | Log filter, see [Logs](#logs) |

`V1PER_MIGRATE` is a switch: `true`, `1`, `yes` or `on` turn it on, `false`, `0`, `no` or `off` leave it off.

## Flags without a variable

| Command | Flag | Meaning |
| --- | --- | --- |
| `init` | `--admin-email` | Email of the instance admin. Required |
| `init` | `--admin-password-file` | File holding the admin password, 12 to 256 characters. Without it a password is generated and printed once |
| `import kuma` | `--into` | The empty node to import into, by ID or path such as `Root/Clients/Acme`. Required |
| `import kuma` | `--dry-run` | Reports what the import would do and writes nothing |
| `import kuma` | `--login-domain` | Domain for Kuma usernames that are not email addresses. Default `kuma.invalid` |
| `import kuma` | `--raw-days` | Days of raw heartbeats to import, 1 to 3650. Default 90 |
| `import kuma` | `--chunk-rows` | Source rows per chunk, 1 to 100000. Default 5000 |
| `backup` | `--out` | The archive to write, a new file. Required |
| `backup` | `--include-key` | Puts the secrets key into the archive |
| `restore` | `--from` | The archive to restore. Required |
| `retention` | `--dry-run` | Counts what would be removed and removes nothing |
| `diagnostics` | `--json` | Prints the report as JSON |
| `upgrade` | `--backup-to` | Where the pre-upgrade backup goes. Required unless an interrupted upgrade resumes |

## Public URL

Set `V1PER_PUBLIC_URL` whenever v1per runs behind a reverse proxy. It does three things:

- Commands from the admin app must come from this origin. Without it they must come from the origin in the request's Host header.
- An `https://` URL marks the session cookie `Secure`.
- Invitation links and status page sign-in links start with it.

Only the scheme and host count, so `https://status.example.com/` and `https://status.example.com` are the same. It must start with `http://` or `https://`.

The instance settings in the admin app also have a public URL field. When `V1PER_PUBLIC_URL` is set, it wins and the field is fixed.

## Secrets key

v1per encrypts every stored secret with one key: monitor credentials and secret headers, TLS keys, notification channel settings, the SMTP password, proxy passwords and two-factor secrets. Each sealed value is bound to the record it belongs to, so a copied value does not open elsewhere.

Some values are hashed rather than encrypted and never need the key: status page passwords, probe tokens, share tokens and API keys.

The key is 32 random bytes. v1per looks for it in this order:

1. `V1PER_SECRETS_KEY`: base64 of the 32 bytes. Read from the environment only, so it never shows in the process list.
2. `V1PER_SECRETS_KEY_FILE` or `--secrets-key-file`: a file holding the 32 raw bytes or their base64.
3. `secrets.key` in the data directory.

Setting both the variable and the file is an error. When neither is set, `init` writes a new `secrets.key` into the data directory with mode 0600. To keep the key outside the data directory, create it first and pass it to `init` and every later command:

```sh
openssl rand -base64 32 > /secure/v1per.key
v1per init --data-dir /var/lib/v1per --admin-email you@example.com --secrets-key-file /secure/v1per.key
```

`serve` never creates a key. If the configured key or `secrets.key` is missing, it refuses to start.

### If the key is lost

Without the key, nothing can read the stored secrets, and a restore refuses to run. You would have to enter every monitor credential, channel setting, SMTP password and second factor again. Store a copy of the key apart from your backups, for example in a password manager. See [Backup](operations.md#backup) for how backups refer to the key.

There is no command to change the key yet. The key you start with stays the key of every backup taken with it.

## Logs

v1per logs to stderr. `V1PER_LOG` sets the filter with the usual `tracing` syntax: a level such as `debug`, or a list such as `info,v1per_server=debug`. The default is `info,sqlx=warn`. `v1per-probe` takes levels and `target=level` pairs only, for example `info,v1per_probe=debug`.

```sh
docker run ... -e V1PER_LOG=debug ghcr.io/coderscantina/v1per:latest
```

Logs hold no secrets. The generated admin password from `init` goes to stdout only.
