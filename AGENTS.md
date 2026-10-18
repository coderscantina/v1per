# v1per agent guide

v1per is a self-hosted uptime monitoring and status page server in Rust. It replaces Uptime Kuma, imports its database, and adds status pages with incident management for a tree of teams and clients.

## Start here

Read [docs/README.md](docs/README.md) and follow its reading order. The technical briefing owns scope, the milestone index owns sequence and status, and the development workflow owns how you take, change and hand back work.

## Rules that apply to every task

- Work from an assigned milestone or brief. State the files you own, the interface you consume, the behavior you add and the check that proves it before editing.
- Stay inside your file ownership. Send changes to shared files (protocol registry, migrations, tokens, locale catalogs) to their owner as a request.
- Make targeted edits. Never hand-edit generated files; regenerate them with `cargo run -p v1per-protocol --bin export-types`.
- Scope changes, relaxed targets and anything touching privacy or secrets go to Mike. Routine implementation choices inside the contract are yours.
- Hand back with the completion report from [docs/development-workflow.md](docs/development-workflow.md#completion-report).
- Do not commit, push or publish unless asked.

## Checks

Once scaffolded (milestone 02 and 03), run the checks for the paths you changed:

- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p v1per-protocol --bin export-types -- --check`
- Web (`apps/web`): `bun run typecheck`, `bun run lint`, `bun run test`, `bun run e2e`

Documentation must not claim a command exists before the code adds it.

## Writing

Plain words, sentence case headings, straight quotes, no em dashes. Gitmoji commits with a lowercase plain-language subject, for example `✨ open an incident from a failing monitor`.
