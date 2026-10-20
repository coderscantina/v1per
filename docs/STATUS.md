# Status

Updated 2026-10-20. Read by agents at the start of a thread; rewrite it before handing back (`project-status` skill).

## Branches

`main` is the only branch, 10 commits ahead of `origin/main` (`58edc2c`) and not pushed. The latest published release is `v26.10.19-ec3e93b`. The release attempt for `58edc2c` (slim probe agent) failed: its release commit only changed `CHANGELOG.md`, CI skipped it, and `release.yml` timed out waiting for CI. The tag was removed. `CHANGELOG.md` now has two `## 26.10.19` sections.

## Recent work

Committed on 2026-10-20 in area commits from three threads (`370b2b8` to the docs commit): tree drag and drop with `monitors.move` and inline folders and renames; quieter forms and desktop-like selection; the incident composer and status page timeline; probe location and platform (`probes.update`, migration `0013_probe_location.sql`); the monitor page around one time frame (four status card designs, History with bars and response time, probe tiles, Details in the right column); Account › Appearance with number, date, time, zone and week formats in `account.preferences`; and a speed round (remembered live snapshots, idle preloading of page code, a 90ms page fade). Shared files went with the area that changed them most, so commits in between may not build on their own; the last one does. At that state the Rust checks, web checks and all 50 e2e tests passed on a fresh build.

Design rules settled along the way are in [design.md](design.md).

Uncommitted, from the 2026-10-20 status page thread, checked (Rust fmt, clippy, server tests, web typecheck, lint, unit tests, all 50 e2e):

- Status page redesign after status.betterstack.com: Status, Maintenance and Incidents views at `{base}`, `{base}/maintenance` and `{base}/incidents` (two new server routes in `pages/public.rs`), a centered hero, a figures row, quiet healthy rows, one time axis per card, and "Powered by V1Per" linking to the repository. Rules in [design.md](design.md#status-page).
- The product name is spelled "V1Per" in prose, UI strings, log and error messages across docs and code. Commands, crate names, env vars, URLs and `CHANGELOG.md` keep `v1per`. The default instance name is now "V1Per".

## Running

The review demo, listed in `.demo/pids`: `v1per serve` on 127.0.0.1:8790 (still on the build before the status page views, so a direct load of `/s/<slug>/incidents` 404s until it restarts; restarted earlier on the new build, data upgraded to control schema 13, backup in `.demo/pre-probe-location-20261020.backup`) with `--web-dir apps/web --public-url http://localhost:5190`, and vite on http://localhost:5190 (5173 and 5174 belong to other projects). Data is Mike's Kuma import in `kuma/demo/data`, channels off. The built-in probe there has a sample location (Vienna, Home lab) to show the feature. The previous demo data is in `kuma/demo/data.prev-20261020`.

## Waiting on Mike

- Push `main`. It holds 10 local commits.
- Three choices from the UX thread. Each has a recommendation in that thread's final report:
  - Dragging a folder when access changes: confirm, or open the Move dialog.
  - Warn when moving a monitor drops its probes, channels and maintenance windows. Recommendation: yes.
  - Whether API keys may call `monitors.move`.
- Release the slim probe agent again. First fix the same-day release hang, either in `scripts/release.sh` or by letting CI run on release commits, and merge the duplicate changelog section.
- Product decisions: 8 open in the [milestone index](milestones/README.md#open-decisions). The Kuma proxies item is settled in practice.

## Next

1. Push, then fix the release path.
2. Bring the milestone index up to date. 03 to 09 have evidence directories but are still listed as in progress with no link.
3. Milestone 12, the integrated release gate.
