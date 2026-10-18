# Milestone 03: Kit shell and generated API

## Outcome

A signed-in instance admin sees the v1per admin shell, served by the Rust binary, talking to the server only through generated command and query resources, with live updates over SSE.

## Dependencies and ownership

Requires 01. Frontend owns `apps/web`. Integration owns `crates/protocol`, the registry, `export-types` and the HTTP command, query and subscribe routes.

## Work

1. Copy holocron's `apps/web` foundation: Vite, bun, Tailwind 4, tokens, Kit components, typed client, Vue Query setup, live snapshot adapter, i18n (en, de), router, test setup. Remove analytics screens.
2. Set up two Vite entries: `src/admin` and `src/status`. The status entry only gets a placeholder here; 09 builds it.
3. Copy the operation registry, typed errors, `export-types` with drift check and OpenAPI output. Add the first operations: session, current user, instance info.
4. Copy holocron's account basics: `v1per init` generating the instance admin with a random password, login, logout, argon2 hashing.
5. Build the shell from holocron's v2 design contract: sidebar, top bar with a node context selector (placeholder until 04), user menu, command palette, mobile layout.
6. Serve built assets from a `web/` directory next to the binary or `V1PER_WEB_DIR`, as holocron does.

## Acceptance criteria

- `export-types -- --check` fails on drift and passes on a clean tree.
- No component calls `fetch` directly.
- The shell passes keyboard, light and dark, desktop and phone checks.
- A fresh `v1per init` and `v1per serve` lets the admin sign in.

## Evidence and handoff

Screenshots and commands in `docs/evidence/m03-kit-shell-and-generated-api.md`. Hand the registry to every backend milestone and the shell to 04.
