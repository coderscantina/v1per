# V1Per agent guide

V1Per is a self-hosted uptime monitoring and status page server in Rust. It replaces Uptime Kuma, imports its database, and adds status pages with incident management for a tree of teams and clients.

## Start here

Read [docs/STATUS.md](docs/STATUS.md) for where the work stands, then [docs/README.md](docs/README.md) and its reading order. The technical briefing owns scope, the milestone index owns sequence and status, and the development workflow owns how you take, change and hand back work.

## Rules that apply to every task

- Work from an assigned milestone or brief. State the files you own, the interface you consume, the behavior you add and the check that proves it before editing.
- Stay inside your file ownership. Send changes to shared files (protocol registry, migrations, tokens, locale catalogs) to their owner as a request.
- Make targeted edits. Never hand-edit generated files; regenerate them with `cargo run -p v1per-protocol --bin export-types`.
- Scope changes, relaxed targets and anything touching privacy or secrets go to Mike. Routine implementation choices inside the contract are yours.
- Hand back with the completion report from [docs/development-workflow.md](docs/development-workflow.md#completion-report) and update `docs/STATUS.md`.
- Do not commit, push or publish unless asked. A subagent in its own worktree commits its checked task to its task branch.

## Checks

Once scaffolded (milestone 02 and 03), run the checks for the paths you changed:

- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p v1per-protocol --bin export-types -- --check`
- Web (`apps/web`): `bun run typecheck`, `bun run lint`, `bun run test`, `bun run e2e`

Documentation must not claim a command exists before the code adds it.

`bun run e2e` serves the built `apps/web/dist` with `target/debug/v1per` (or `V1PER_BIN`) and never rebuilds either. Run `bun run build` and `cargo build -p v1per-server` after changes, or the suite tests the old build.

## Gotchas

- Storage refuses writes below a 4 GiB disk reserve (`crates/storage/src/lib.rs`). e2e and import failures on a nearly full disk are a disk problem first. Rust `target/` directories are the usual cause; parallel worktrees share one `CARGO_TARGET_DIR`.
- The probe agent is its own binary, `v1per-probe`. `v1per probe` no longer exists.
- `scripts/release.sh` releases a second time on the same day with a commit that touches only `CHANGELOG.md`. CI ignores Markdown pushes, so `release.yml` waits for a CI run that never starts. Avoid same-day re-releases until the script or workflow handles it.

## Demo

Used by the `demo` skill. Mike's demo means his real Kuma database unless he asks for the seeded one.

- Real data: `kuma/kuma.db` (gitignored, read-only source), scratch data in `kuma/demo/data`. Build with `cargo build -p v1per-server` and `bun run build` in `apps/web`. Then run `v1per init --data-dir kuma/demo/data --admin-email <email> --admin-password-file <file>`, then `v1per import kuma --data-dir kuma/demo/data --source kuma/kuma.db --into Root`. Switch notification channels off while stopped: `sqlite3 kuma/demo/data/control.sqlite "UPDATE channels SET active = 0"`. Serve with `v1per serve --data-dir kuma/demo/data --bind 127.0.0.1:8790 --web-dir apps/web/dist`. The import manifest `import-node-<id>.json` lists the imported logins.
- Seeded data: `bun scripts/demo-seed.ts [--dir /tmp/v1per-demo] [--port 8097]` builds the synthetic Northwind agency used for screenshots (`bun scripts/demo-shots.ts`). The login is in its `demo.json`.

## Writing

Plain words, sentence case headings, straight quotes, no em dashes. Gitmoji commits with a lowercase plain-language subject, for example `✨ open an incident from a failing monitor`.
