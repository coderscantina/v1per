# Milestone 09: status pages

## Outcome

Admins publish status pages for the public, clients or their own team, and visitors see current status, 90 days of history and incidents, updated live, on the instance or a custom domain.

## Dependencies and ownership

Requires 04, 05 and 08. Integration owns `pages` and the public routes. Frontend owns `apps/web/src/status` and the page editor in the admin app.

## Work

1. Implement pages per [0004](../decisions/0004-status-pages-and-incidents.md): slug, title, description, logo, theme, locale, components from the node's subtree, static components, groups, order, auto-incident mode and grace.
2. Implement access modes: public, node grant login, page password (argon2, rate-limited), magic link to allowed emails or domains via SMTP, share tokens with expiry and revocation. A page combines restricted modes.
3. Serve `/s/{slug}` and claimed hostnames with the initial page JSON injected into the HTML. Serve a per-page SSE stream with coalesced updates.
4. Build the snapshot per audience: public and client viewers get status, bars, uptime and published incidents; grant holders also get response-time charts and internal components.
5. Build the status page app: header, overall status, active incidents and maintenance, groups with components and 90 daily bars, incident history, en and de, light and dark, theming from page settings.
6. Add SVG badges (status, uptime) for public pages and share tokens.

## Acceptance criteria

- No public or client response contains internal fields, other nodes or draft incidents; tested per access mode.
- The status page bundle stays separate from the admin bundle and meets the page target.
- A status change reaches an open public page within the target, and a reconnect restores the snapshot.
- Desktop, phone, light, dark, keyboard and screen reader checks pass.

## Evidence and handoff

Access matrix tests, bundle sizes, timings and screenshots in `docs/evidence/m09-status-pages.md`. Hand the page model to 10.
