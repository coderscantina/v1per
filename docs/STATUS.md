# Status

Updated 2026-11-12. Read by agents at the start of a thread; rewrite it before handing back (`project-status` skill).

## Branches

`main` is the only branch, one commit ahead of `origin/main`: `c718657`, a fix that lets `diagnostics` read retention runs stored before 26.11.12. It is not pushed or released. No worktrees. The latest release is `v26.11.12-b392970` (2026-11-12); CI needed three runs because of the flaky crash tests listed under Next.

Merged into `main` on 2026-11-12: milestone 14 (SLAs, evidence in `docs/evidence/m14-slas.md`) and the status page upgrades of [decision 0013](decisions/0013-status-page-upgrades.md):

- Response times for visitors as a page setting, per probe location as a second one, with 24 hour, 7, 30 and 90 day ranges, p50, p95 and p99, and incidents and maintenance marked on the chart (`response-times.json`).
- Page subscribers: email with double opt-in, Atom and RSS feeds, a maintenance calendar, and webhooks added by page managers.
- Incident pages at `/incidents/<id>`, postmortems, link preview tags.
- Check counts behind every uptime figure and a "How uptime is counted" block.

Migrations 0014 to 0018 move the control schema from 13 to 18. Storage gains a `compacted_days` marker table without a version bump.

Checked on the merged tree before the merge to `main`, which then added docs only: fmt, clippy, `export-types --check`, `cargo test --workspace` (72 test binaries, all passing), web typecheck, lint and 162 unit tests, and the full e2e suite (59 tests; the probe spec passed on a rerun once `v1per-probe` was built).

## Recent work

Committed on 2026-10-20 in area commits from three threads (`370b2b8` to the docs commit): tree drag and drop with `monitors.move` and inline folders and renames; quieter forms and desktop-like selection; the incident composer and status page timeline; probe location and platform (`probes.update`, migration `0013_probe_location.sql`); the monitor page around one time frame (four status card designs, History with bars and response time, probe tiles, Details in the right column); Account › Appearance with number, date, time, zone and week formats in `account.preferences`; and a speed round (remembered live snapshots, idle preloading of page code, a 90ms page fade). Shared files went with the area that changed them most, so commits in between may not build on their own; the last one does. At that state the Rust checks, web checks and all 50 e2e tests passed on a fresh build.

Design rules settled along the way are in [design.md](design.md).

Also committed on 2026-10-20: `c394f8c` the status page in Status, Maintenance and Incidents views, and `43719a9` the "V1Per" spelling across docs and code.

Committed in `93a45ee`, from the 2026-10-20 PWA thread: the admin app installs to the home screen. `apps/web/public` has the manifest, icons from Mike's favicon, and the favicon itself. `apps/web/service-worker.js` is built into `dist/sw.js` by a plugin in `vite.config.ts` that lists the files to precache; it serves the shell offline, sends `/api`, `/config.json` and `/s/` to the network, and only runs in production builds. The theme color follows the color mode. App.vue shows a "you're offline" screen while the first navigation waits for the network. AppBrand draws the V1P logo in the accent color. Rule in [design.md](design.md#brand). Checked: web typecheck, lint, unit tests, all 50 e2e, and a Playwright run against a built server (no installability errors, 138 files precached, offline screen, status pages bypass the worker).

`fc1761c` makes `ci.yml` run for `CHANGELOG.md`, so a second release on the same day no longer hangs. Not yet exercised by a real same-day release.

## Running

Nothing local. The review demo is stopped; its data is in `kuma/demo/data` (control schema 13, needs `v1per upgrade` before the next start). Production runs 26.11.12 on ops01.coderscantina.com (https://monitoring.coderscantina.com) under Coolify from `ghcr.io/coderscantina/v1per:latest`, with `V1PER_MIGRATE=true`. Deployed on 2026-11-12 by `docker compose pull` and `up -d` in `/data/coolify/services/nodkztgpbc5uiss108enwhyn` over ssh as `ubuntu`; the control schema went from 13 to 18 at start. The backup taken just before is `~ubuntu/backups/v1per-pre-26.11.12.tar` on the host, without the secrets key. Until the first hourly retention run after the deploy, `v1per diagnostics` there fails on the old stored run; `c718657` fixes that for later upgrades.

## Waiting on Mike

- The amendment to decision 0002: status page percentiles read raw heartbeats, compacted once per closed day. Recommendation: accept.
- An outbound address policy for webhooks. None exists for page webhooks or webhook notification channels.
- SLA edit rule reach: a client Admin on `autohaus` can edit SLAs on `schmidt` below it. Recommendation: keep the strict-ancestor rule and put client SLAs on the client's top node.
- SLA alert language: taken from the first status page showing the SLA, else English. Recommendation: a language setting on channels later.
- Three choices from the UX thread. Each has a recommendation in that thread's final report:
  - Dragging a folder when access changes: confirm, or open the Move dialog.
  - Warn when moving a monitor drops its probes, channels and maintenance windows. Recommendation: yes.
  - Whether API keys may call `monitors.move`.
- Product decisions: 8 open in the [milestone index](milestones/README.md#open-decisions). The Kuma proxies item is settled in practice.

## Next

1. Bring the milestone index up to date. 03 to 09 have evidence directories but are still listed as in progress with no link, and the status page upgrades have no milestone entry.
2. Follow-ups from the upgrade work: `scripts/demo-seed.ts` still calls `v1per probe`; one SMTP connection per subscriber mail; "Only some components" with none ticked subscribes to everything; German mails say "Sie" while the page says "du"; the first series colour is close to the overall line on blue accents.
3. Flaky crash tests on the CI runners, seen before this work too: `an_import_killed_in_each_step_resumes_without_duplicates` (the server's read-only inbox connection meets a database that still needs recovery, SQLite code 776) and the `failpoints` tests (`child was not killed`).
4. `AGENTS.md` says parallel worktrees share one `CARGO_TARGET_DIR`. That let one branch's build reuse another's crates and filled the disk on 2026-11-11. Give each worktree its own.
5. Milestone 12, the integrated release gate.
