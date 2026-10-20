# Changelog

All notable changes to v1per are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## 26.10.20

Remote probe agents are now their own small program, `v1per-probe`: a static binary of 7 to 8 MB and the `ghcr.io/coderscantina/v1per-probe` image, built on an empty base. An idle agent uses about 8 MiB of memory, half of before, and writes nothing to disk while the server answers. `v1per probe` is gone.

**Upgrading an agent:** check on the Probes page that the probe shows no buffered results, stop the old agent, then start `v1per-probe` with the same token, server and data directory. With docker compose, take the new compose file: its probe service uses the new image. An old compose file's probe service keeps restarting, because the server image no longer has `v1per probe`. The server and its built-in probe are not affected. The new agent does not read the old `buffer.sqlite`. See [Probes](docs/guide/probes.md#upgrade-an-agent-from-v1per-probe).

- **Monitor page:** one time frame from 1 hour to 90 days drives a History panel with availability bars and response times, probe tiles show each probe's latest checks, and the status block comes in four designs
- **Status pages:** separate Status, Maintenance and Incidents views, with incidents shown as a timeline
- **Incidents:** one composer sets what an incident affects and how badly
- **Tree:** drag monitors and folders to arrange or move them, create folders and rename rows in place
- **Probes:** set a probe's country, city and provider, and see the platform each agent runs on
- **Account › Appearance:** theme, accent, language, time zone and number, date, time and week formats
- **Install the admin app:** add it to the home screen, with an offline screen while the network is away
- **Faster views:** reopened views show their last data at once while fresh data loads

### Features

- ✨ ship the probe agent as its own small binary
- ✨ arrange monitors and folders by dragging them in the tree
- ✨ write incidents in one composer and show them as a timeline
- ✨ say where each probe runs: country, city and provider
- ✨ redesign the monitor page around one time frame
- ✨ set appearance, language and formats on an account page
- ✨ split the status page into status, maintenance and incident views
- ✨ install the admin app to the home screen

### Performance

- ⚡️ open views instantly and keep layouts still while they load

### Interface

- 💄 quieter forms that select like a desktop app

### Other changes

- 🙈 ignore demo data and agent worktrees
- 🔧 serve status pages through the vite dev proxy
- 📝 track project status and the settled design rules
- ✏️ spell the product name V1Per
- 👷 run ci for changelog-only release commits

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
- 🐛 regenerate the protocol contract when a release bumps the version

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
- 🐳 give the docker build the kuma schema the importer embeds

