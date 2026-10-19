# v1per user guide

v1per is a self-hosted uptime monitoring and status page server. One process with one data directory runs your checks, sends alerts and serves status pages for every team and client you look after.

This guide is for the people who install and run an instance, and for the admins who set up monitors, channels and pages inside it.

## Pages

| Page | What it covers |
| --- | --- |
| [Install](install.md) | Docker, docker compose, Coolify, a reverse proxy with Caddy, `v1per init` and the first sign-in |
| [Configuration](configuration.md) | Every command, flag and `V1PER_*` environment variable, and the secrets key |
| [Migrate from Uptime Kuma](kuma-migration.md) | `v1per import kuma`, what it brings over, push URLs and a cutover checklist |
| [Probes](probes.md) | The built-in probe, remote probe agents and how the quorum decides a monitor's status |
| [Status pages](status-pages.md) | Access modes, custom domains, badges, languages, incidents, maintenance and who sees what |
| [Notifications](notifications.md) | Channel setup per provider, the webhook payload, the test button and the delivery log |
| [Operations](operations.md) | Backup, restore, upgrade, retention, diagnostics, the data directory, health and logs |

## Concepts in one minute

- **Nodes.** Everything lives in a tree of nodes: your agency at the root, then clients, teams or projects below. Monitors, channels, probes and status pages belong to a node.
- **Roles.** People get a role on a node: Owner, Admin, Member or Viewer. A role applies to that node and everything below it. Someone invited to a client's node never sees the other clients.
- **Monitors.** Supported types are http, keyword, json-query, port, ping, dns, push, group and manual. Http monitors can also warn before a TLS certificate expires.
- **Probes.** The server checks monitors itself. Remote probe agents run the same image elsewhere and check from other networks.
- **Status pages.** A page shows monitors and static components from its node and the nodes below, with incidents and maintenance.

## Getting help

`v1per --help` and `v1per <command> --help` list every option. `v1per diagnostics` prints the state of an instance without any credentials, so its output can go into a bug report.
