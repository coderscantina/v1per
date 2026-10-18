# v1per

<div align="center">

**Self-hosted uptime monitoring and status pages.** One server for many teams and clients, with incident management built in. Replaces Uptime Kuma and imports its data. Written in Rust, licensed AGPL-3.0.

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-pre--alpha-orange.svg)](CHANGELOG.md)
[![CI](https://github.com/coderscantina/v1per/actions/workflows/ci.yml/badge.svg)](https://github.com/coderscantina/v1per/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/coderscantina/v1per?include_prereleases)](https://github.com/coderscantina/v1per/releases)

</div>

## Status

v1per is pre-alpha and under development. Nothing is built yet, so there is nothing to install or run. This repo currently holds the project files and design docs. Everything below describes what v1per is planned to do.

## What it is

v1per watches your services, tells people when they break, and publishes status pages. It is a single process with a data directory, shipped as one binary and a Docker image.

It started as a replacement for [Uptime Kuma](https://github.com/louislam/uptime-kuma). It adds status pages and incident management in the style of [OpenStatus](https://www.openstatus.dev/), and it serves many teams and clients from one instance.

## Planned features

- **A tree of nodes.** One instance serves many teams and clients through a tree of nodes with any depth. Invite people at any node and they see its subtree.
- **Monitors.** HTTP, keyword, JSON query, port, ping, DNS, push, group, manual, and TLS certificate expiry.
- **Probes.** A built-in probe runs checks. Remote probe agents (`v1per probe`, same binary, outbound HTTPS only) add more vantage points. A majority quorum across probes decides the result.
- **Status pages.** Public, login, password, magic link, or share token access. Custom domains, 90 daily uptime bars, live updates, badges, English and German.
- **Incidents.** Manual incidents with an update timeline and per-component impact, auto-incidents opened by monitors, and scheduled maintenance.
- **Notifications.** Email (SMTP), Discord, webhook, Slack, Teams, Telegram, ntfy, and Pushover.
- **Uptime Kuma import.** A one-shot import from an Uptime Kuma 2.5.0 SQLite file, including heartbeat history. Existing push URLs (`/api/push/:token`) stay compatible.

## Planned usage

Import from Uptime Kuma into a node:

```bash
v1per import kuma --source kuma.db --into <node> --dry-run
```

Run a remote probe agent:

```bash
v1per probe --server https://status.example.com --token <probe-token>
```

Install steps and configuration (env vars use the `V1PER_` prefix) will be documented once there is something to install.

## Technical architecture

- **Backend**: Rust with Axum
- **Database**: SQLite through SQLx, plus embedded DuckDB
- **Web UI**: Vue 3, Vite, Tailwind 4, built with bun
- **Deployment**: one process, one data directory, a Docker image

## Development

The layout is a Cargo workspace at the repo root (`apps/server`, `crates/*`) and a bun frontend in `apps/web`. The scaffold has not landed yet. See the [Contributing guide](CONTRIBUTING.md) for conventions, commit format, and the PR process.

## Security

Please report vulnerabilities privately. See the [Security policy](SECURITY.md).

## Community

- [GitHub Discussions](https://github.com/coderscantina/v1per/discussions) for questions, ideas, and technical discussions
- [GitHub Issues](https://github.com/coderscantina/v1per/issues/new/choose) for bugs and feature requests

## License

v1per is licensed under the [GNU AGPLv3](LICENSE).

## Acknowledgements

- Louis Lam and contributors for Uptime Kuma, which this project replaces and imports from
- The OpenStatus team for the status page ideas
- The open source community behind the Rust and Vue libraries used by this project

---

<div align="center">
  <p>Maintained by <a href="https://github.com/badmike">Michael Wallner</a> at Coder's Cantina</p>
</div>
