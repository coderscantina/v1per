# Status

Updated 2026-10-20. Read by agents at the start of a thread; rewrite it before handing back (`project-status` skill).

## Branches

`main` is the only branch, 12 commits ahead of `origin/main` (`58edc2c`) and not pushed. The latest published release is `v26.10.19-ec3e93b`. The release attempt for `58edc2c` (slim probe agent) failed: its release commit only changed `CHANGELOG.md`, CI skipped it, and `release.yml` timed out waiting for CI. The tag was removed.

## Recent work

Committed on 2026-10-20 in area commits from three threads (`370b2b8` to the docs commit): tree drag and drop with `monitors.move` and inline folders and renames; quieter forms and desktop-like selection; the incident composer and status page timeline; probe location and platform (`probes.update`, migration `0013_probe_location.sql`); the monitor page around one time frame (four status card designs, History with bars and response time, probe tiles, Details in the right column); Account › Appearance with number, date, time, zone and week formats in `account.preferences`; and a speed round (remembered live snapshots, idle preloading of page code, a 90ms page fade). Shared files went with the area that changed them most, so commits in between may not build on their own; the last one does. At that state the Rust checks, web checks and all 50 e2e tests passed on a fresh build.

Design rules settled along the way are in [design.md](design.md).

Also committed on 2026-10-20: `c394f8c` the status page in Status, Maintenance and Incidents views, and `43719a9` the "V1Per" spelling across docs and code.

Uncommitted, from the 2026-10-20 PWA thread: the admin app installs to the home screen. `apps/web/public` has the manifest, icons from Mike's favicon, and the favicon itself. `apps/web/service-worker.js` is built into `dist/sw.js` by a plugin in `vite.config.ts` that lists the files to precache; it serves the shell offline, sends `/api`, `/config.json` and `/s/` to the network, and only runs in production builds. The theme color follows the color mode. App.vue shows a "you're offline" screen while the first navigation waits for the network. AppBrand draws the V1P logo in the accent color. Rule in [design.md](design.md#brand). Checked: web typecheck, lint, unit tests, all 50 e2e, and a Playwright run against a built server (no installability errors, 138 files precached, offline screen, status pages bypass the worker).

Uncommitted, from the 2026-10-20 release thread: `ci.yml` now runs for `CHANGELOG.md`, so a second release on the same day no longer hangs. The stale `## 26.10.19` probe section is gone from `CHANGELOG.md`; the next release lists the probe again. The AGENTS.md gotcha says how same-day releases behave now. The highlights for the next release are in `/tmp/v1per-release-highlights.md` (it covers the PWA work too). Checked: actionlint and `scripts/release.sh --dry-run`.

## Running

The review demo, listed in `.demo/pids`: `v1per serve` on 127.0.0.1:8790 (still on the build before the status page views, so a direct load of `/s/<slug>/incidents` 404s until it restarts; restarted earlier on the new build, data upgraded to control schema 13, backup in `.demo/pre-probe-location-20261020.backup`) with `--web-dir apps/web --public-url http://localhost:5190`, and vite on http://localhost:5190 (5173 and 5174 belong to other projects). Data is Mike's Kuma import in `kuma/demo/data`, channels off. The built-in probe there has a sample location (Vienna, Home lab) to show the feature. The previous demo data is in `kuma/demo/data.prev-20261020`.

## Waiting on Mike

- Push `main`. It holds 12 local commits.
- Three choices from the UX thread. Each has a recommendation in that thread's final report:
  - Dragging a folder when access changes: confirm, or open the Move dialog.
  - Warn when moving a monitor drops its probes, channels and maintenance windows. Recommendation: yes.
  - Whether API keys may call `monitors.move`.
- Commit the PWA work and the release fix, then release with `scripts/release.sh /tmp/v1per-release-highlights.md`. That also pushes the local commits.
- Product decisions: 8 open in the [milestone index](milestones/README.md#open-decisions). The Kuma proxies item is settled in practice.

## Next

1. Release 26.10.20.
2. Bring the milestone index up to date. 03 to 09 have evidence directories but are still listed as in progress with no link.
3. Milestone 12, the integrated release gate.
