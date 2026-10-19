# Probes

A probe runs checks. Every instance has a built-in probe inside the server. Remote probe agents run `v1per-probe`, a small separate program, on other machines, so you can check a service from another region or from inside a client's private network.

Push, group and manual monitors have no probe. The server evaluates them itself.

## The built-in probe

The built-in probe lives on the root node and runs inside `v1per serve`. A monitor with no probes picked runs on it. It needs no setup.

## Remote probe agents

An agent only opens outbound HTTPS connections to the server. It listens on nothing, so it works behind NAT and needs no open port or firewall rule beyond outbound HTTPS.

A probe registered on a node serves monitors on that node and every node below it. Register instance-wide probes on the root. Register a probe on a client's node to reach that client's network for that client alone.

### Register a probe

1. Open the node's Probes page in the admin app. Adding and revoking probes needs the Admin role on the node.
2. Choose **Add probe** and give it a name, such as where it runs: `Frankfurt` or `Acme office`.
3. Copy the token and the command shown. The token is shown once and stored only as a hash.

The probe shows as Waiting until the agent connects, then Online.

### Run an agent

Run the agent of the same release as the server. Pass the token as an environment variable. A `--token` flag works too, but shows in the process list.

With Docker, use the `ghcr.io/coderscantina/v1per-probe` image for linux/amd64 and linux/arm64. It holds only the agent, a few MB:

```sh
docker run -d --name v1per-probe --restart unless-stopped \
  -v v1per-probe-data:/probe-data \
  -e V1PER_PROBE_SERVER=https://status.example.com \
  -e V1PER_PROBE_TOKEN=... \
  ghcr.io/coderscantina/v1per-probe:latest
```

The image has no healthcheck. The Probes page and the probe offline alert show whether the agent is in contact.

Without Docker, download the static binary for your architecture from the release on GitHub, `v1per-probe-<version>-linux-amd64` or `-linux-arm64`, and check it against its `.sha256` file. It needs no libraries:

```sh
chmod +x v1per-probe-*-linux-amd64
V1PER_PROBE_TOKEN=... ./v1per-probe-*-linux-amd64 --server https://status.example.com --data-dir /var/lib/v1per-probe
```

Run it under your service manager, for example systemd with `Restart=on-failure`.

With docker compose, put the token in `V1PER_PROBE_TOKEN` and start the probe profile of [deploy/docker-compose.yml](../../deploy/docker-compose.yml):

```sh
docker compose --profile probe up -d probe
```

`V1PER_PROBE_SERVER` defaults to `V1PER_PUBLIC_URL` there. All agent settings are listed in [Configuration](configuration.md#environment-variables).

### Use a probe on a monitor

In the monitor editor, pick the probes under **Probes**. You can pick probes on the monitor's node and on the nodes above it. With none picked, the built-in probe runs the check.

### Buffering

The agent sends its results oldest first. While the server answers, they wait in memory and nothing is written to disk. While the server is unreachable, results go to the `buffer` directory in the agent's data directory (`/probe-data` in the image) and arrive once the server is back. Mount a volume on the data directory to keep them across restarts.

- The buffer holds up to `--buffer-mb` MiB on disk, 64 by default. When it is full, the oldest results are dropped and the count is reported to the server.
- If the agent process crashes while the server answers, it loses the results of its last few seconds at most.
- Only one agent at a time can use a data directory.
- The agent keeps check credentials in memory only. An agent started while the server is unreachable checks nothing until it gets its list.

Results that waited longer than one check interval are kept in that probe's history but do not change the monitor's status. After an outage the monitor shows a gap rather than a guess.

### Probe status

The Probes page shows each probe's status, version, last contact, lag and monitor count.

| Status | Meaning |
| --- | --- |
| Waiting | Registered, never connected |
| Online | In contact |
| Offline | No contact for 90 seconds. Active channels on the probe's node get a "probe offline" alert |
| Revoked | Its token no longer works |

The page also flags a probe whose clock differs from the server's by more than 5 seconds, checks that started late, and results waiting in or dropped from its buffer.

**Revoke** ends a token at once. The agent is refused at its next request and exits with status 3, and its monitors stop using it. This cannot be undone. Register a new probe instead.

An agent from a different release is refused with a message naming both protocol versions. Run the same release on the server and its agents.

### Ping monitors and ICMP

Ping monitors need ICMP. The agent uses unprivileged ping sockets, which Docker allows in containers by default. If your host restricts it, add `--cap-add NET_RAW` to the agent container, or uncomment `cap_add: [NET_RAW]` in the compose file.

## How the quorum decides a monitor's status

Each probe runs its own retries: a failed check is Pending until the monitor's retries are used up, then Down. The monitor's status then follows its **Probe quorum**, counted over the live probes that run it:

| Quorum | Down when |
| --- | --- |
| Half or more (default) | at least half of the live probes report down |
| Any probe | one live probe reports down |
| Every probe | all live probes report down |

Pending follows the same rule over probes that report down or pending.

A probe is live for a monitor while it is online, not revoked, still picked, and its latest result for the monitor is no older than three check intervals. A probe that drops out leaves the count, so one offline probe does not turn a monitor down. With no live probe at all the monitor is Pending with the message "No live probe".

Uptime is not multiplied by the number of probes. The monitor keeps one entry per check round, plus one for each status change. Per-probe status and latency show on the monitor's page and to people with a role on the node.

## Upgrade an agent from `v1per probe`

Earlier releases ran the agent as `v1per probe` from the server image, buffering in `buffer.sqlite`. `v1per-probe` does not read that file.

1. Check on the Probes page that the probe shows no buffered results, then stop the old agent.
2. Start `v1per-probe` with the same token, server and data directory. With docker compose, pull the new compose file: its probe service uses the `v1per-probe` image.
3. The new agent logs a warning while `buffer.sqlite` is there. Delete the file once you no longer need it.

