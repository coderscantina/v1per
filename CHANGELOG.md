# Changelog

All notable changes to v1per are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## 26.10.19

The first public release of v1per, in beta. One self-hosted instance monitors every client you look after, with status pages, incidents and remote probes.

- **Node tree and access:** nested nodes for your agency, clients and teams, invitations, roles and two-factor sign-in
- **Monitors:** HTTP, keyword, JSON query, TCP port, ping, DNS, push, group, manual and TLS certificate expiry, checked by a built-in probe and remote agents with a majority quorum
- **Status pages:** per client, on custom domains, with public, sign-in, password, magic link or share token access, in English and German
- **Incidents and maintenance:** update timelines, incidents opened by monitors, scheduled maintenance windows
- **Notifications:** email, Slack, Discord, Microsoft Teams, Telegram, ntfy, Pushover and webhooks
- **Uptime Kuma import:** Kuma 2.5.0 databases with heartbeat history; push URLs keep working

Run it with `docker pull ghcr.io/coderscantina/v1per:latest`. See the [install guide](https://github.com/coderscantina/v1per/blob/main/docs/guide/install.md).

### Features

- 🎉 "this is where the fun begins": brief v1per
- ✨ store results in a sqlite inbox and heartbeats in duckdb
- ✨ run kuma-compatible checks
- ✨ send email, discord and the other core notifications
- ✨ compute maintenance windows from kuma schedules
- ✨ import a kuma database with dry run and reconciliation
- ✨ serve monitors, nodes, status pages, incidents and probes
- ✨ add the admin app and status pages
- ✨ navigate a tree of nodes and monitors, and observe or configure each node
- ✨ keep a neon accent in each person's preferences
- ✨ test a notification channel with a real alert
- ✨ read a node's last 24 hours by the hour
- ✨ show the last 24 hours under each dashboard stat
- ✨ add an extended wall with each monitor's last day

### Fixes

- 🐛 give the add-child-node field an id of its own
- 🐛 set the log level the server reads in the compose files

### Performance

- ⚡️ add a synthetic workload for the storage gate
- ⚡️ cache the rust build across failed ci runs and skip ci for docs

### Interface

- ♿️ name the close button of side sheets
- 💄 raise the chosen option in segmented controls
- 💄 quieten the orbit and name paused monitors in its legend
- 💄 draw alerts as a glowing rail

### Other changes

- 📝 settle monitoring rules, storage budgets and operations in the briefing
- 🏗️ set up the rust workspace and protocol registry
- 🧪 generate a deterministic kuma 2.5.0 fixture
- 🐳 package v1per for docker, coolify and caddy
- 📝 record milestone evidence
- 🔥 drop the live indicator
- 🔧 regenerate the protocol contract
- 🔧 show the release commit in v1per --version
- 👷 cut calver releases with gitmoji release notes
- 📝 add a user guide for install, migration and operations
- 📸 seed a demo agency and capture the readme visuals
- 📝 present v1per as a beta for agencies and msps
- 💚 allow unprivileged ping on the ci runner
- 👷 build release images on native runners and reuse the ci run

