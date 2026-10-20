# Status page API

Milestone 09. This page owns the public status page routes and the snapshot shape. The admin operations (`pages.*`, `incidents.publish`, `incidents.unpublish`, `incidents.publications`) are in the operation registry and the [capability matrix](capability-matrix.md). Wire types live in `crates/protocol/src/status_page.rs` and are generated into `apps/web/src/generated` (`PageBoot`, `PageSnapshot`, `PageIncidents`, `PageGate`, `PagePasswordBody`, `PageEmailBody`, `PageRedeemBody`).

## Where pages are served

A page has a base path:

- On the instance's own host: `/s/{slug}`.
- On a hostname the page claimed with `pages.claimHostname`: the empty string, so the page is the host's root.

The server routes a request by its `Host` header (port ignored) before anything else. A claimed hostname serves only the page routes below, `/assets/*` and `/favicon.svg`; the admin app, `/api`, `/config.json` and other pages answer `404`. The host of `V1PER_PUBLIC_URL` is never routed to a page, and `pages.claimHostname` refuses the configured or stored public URL's host. TLS for claimed hostnames is the reverse proxy's job.

## Routes

All paths are relative to the base. Errors use the admin API's `ApiErrorBody`.

| Method and path | Answer |
| --- | --- |
| `GET {base}` | `status.html` with the [boot JSON](#boot-json) injected |
| `GET {base}/t/{token}` | The same, opened with a share token |
| `GET {base}/incidents`, `GET {base}/maintenance` | The same, opened on that view |
| `GET {base}/access/email?code=` | The same, without using the code; the page app posts `code` to `redeem` once the visitor continues |
| `GET {base}/snapshot.json` | `PageSnapshot`, or `401 unauthenticated` when the page is closed to the visitor |
| `GET {base}/incidents.json?cursor=&limit=` | `PageIncidents`, see [incident history](#incident-history) |
| `GET {base}/events?cursor=page:{id}@{seq}` | Server-sent events, see [stream](#stream) |
| `GET {base}/badge/{name}` | SVG badge, see [badges](#badges) |
| `GET {base}/logo` | The page logo, `404` without one |
| `POST {base}/access/password` | `{ password }`: `204` with the page cookie, `401 invalid_credentials` |
| `POST {base}/access/email` | `{ email, locale? }`: `202 {}` whether or not the address is allowed |
| `POST {base}/access/email/redeem` | `{ code }`: `204` with the page cookie, `401 invalid_credentials` |

Every route except the badges and the logo takes `?token=<share token>`. The HTML and JSON routes take `?locale=en|de`; without it the server picks from `Accept-Language` among the page's enabled locales, then the page's default.

The `POST` routes need an `Origin` naming the page's own origin (`origin_rejected` otherwise) and a JSON body of at most 4 KiB. Password attempts, sign-in link requests and redemptions together are limited to 10 per minute per client address, and links to 3 per minute per recipient (`429 rate_limited`). There is no per-page limit, so guessing cannot lock visitors out of a page.

The logo upload is an admin route: `POST /api/pages/{page}/logo` with the image as the raw body, at most 512 KiB, from a signed-in person with `pages.manage` and the request marker and same-origin `Origin` of commands. PNG, JPEG, GIF, WebP and SVG are recognized by their bytes; anything else is `invalid_arguments` on `logo`. It answers like a command: `PageDetail` with `revision`. `pages.removeLogo` deletes it. Logos are served with `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`, `X-Content-Type-Options: nosniff` and a one-day cache; their path carries the upload time as `?v=`.

## Access

| Visitor | Audience |
| --- | --- |
| A signed-in person whose scope resolves on the page's node (Viewer and up, on the node or an ancestor) | `internal`, on any page |
| Anyone, on a public page | `public` |
| A valid share token in the request, or a page cookie from a share token, the page password or a sign-in link | `public` |
| Anyone else | closed: the HTML carries a gate, the JSON routes answer `401 unauthenticated` |

Grant holders sign in on the instance, so grant access works at `/s/{slug}` only, not on claimed hostnames. A restricted page without a password, allowed addresses or share tokens is a team-internal page.

The page cookie is `v1per_page`, `HttpOnly`, `SameSite=Lax`, `Secure` when `V1PER_PUBLIC_URL` is https, with `Path` set to the base (or `/`). It lasts 30 days, a share token's cookie no longer than its token. It is checked against the page's current settings on every request:

- A password cookie ends when the password is changed or cleared.
- A sign-in link cookie ends when its address is no longer allowed.
- A share token cookie ends when the token is revoked or expires.

Opening the page with a valid share token on a restricted page sets a token cookie, so the app's later requests need no token.

Sign-in links go to addresses in the page's `emails` list or whose domain is in `domains`, through the instance SMTP server. A link is `{origin}{base}/access/email?code=<code>`, works once and for 15 minutes. Its origin is the claimed hostname, or the instance's public URL (`V1PER_PUBLIC_URL` or the stored setting); without a public URL no link is sent on the instance host, so a forged `Host` cannot point a link elsewhere. Opening the link never uses the code: `GET {base}/access/email?code=` only serves the page app, and only `POST {base}/access/email/redeem` consumes it. The page app must post the code only after the visitor confirms with a Continue step, so neither a mail scanner that follows links nor one that runs scripts uses it up.

## Boot JSON

The HTML carries `PageBoot` in `<script id="v1per-page" type="application/json">` before `</head>`. `<`, `>`, `&`, U+2028 and U+2029 are escaped as `\uXXXX`, so no value can end the element. The server also sets `<html lang>` to the chosen locale and `<title>` to the page title.

```ts
type PageBoot =
  | { kind: 'page'; base: string; snapshot: PageSnapshot }
  | { kind: 'gate'; base: string; gate: PageGate }

type PageGate = {
  appearance: PageAppearance
  locale: Locale
  password: boolean // POST {base}/access/password works
  email: boolean // POST {base}/access/email sends links
  sign_in_url?: string // /login?redirect=/s/{slug}, absent on a claimed host without a public URL
}
```

## Snapshot

```ts
type PageSnapshot = {
  appearance: { title; theme: 'auto' | 'light' | 'dark'; accent: Accent; logo?: string; default_locale; locales }
  description: string
  footer: string
  timezone: string // IANA zone of the bars
  locale: 'en' | 'de' // what incident titles and server texts are in
  audience: 'public' | 'internal'
  status: PageStatus // worst visible component
  groups: { id; name; default_open; components: PublicComponent[] }[]
  incidents: PublicIncident[] // newest first
  maintenance: PublicMaintenance[]
  cursor: string // page:{id}@{seq}, for the stream
  generated_at: number
}

type PageStatus = 'operational' | 'maintenance' | 'degraded' | 'partial_outage' | 'major_outage'

type PublicComponent = {
  id: PageComponentId
  name: string // display name, or the monitor's or static component's name
  description: string // static components only
  status?: PageStatus // absent while a monitor has no status, such as when paused
  uptime_90d?: number // ratio over the 90 bars, monitors only
  bars: { day; status?; uptime?; utc_approximate?; maintenance?; incidents?: IncidentId[] }[] // 90 days, oldest first
  detail?: { latency: LatencyPoint[]; probes: { name; status; latency_ms?; checked_at }[] } // internal only
}

type PublicIncident = {
  id; title; state; impact?; started_at; resolved_at?
  components: PageComponentId[]
  updates: { state; message; created_at; impacts: { component: PageComponentId; impact }[] }[] // newest first
}

type PublicMaintenance = { title; description; active: boolean; starts_at?; ends_at?; components: PageComponentId[] }
```

Rules:

- The `public` audience sees only components not marked `internal`; groups left empty are dropped. The `internal` audience sees every component and gets `detail` on monitor components: hourly response times over the last 24 hours and each assigned probe's status. While a monitor has fewer than two hourly points, as a new one does, its response times are per minute over the last 2 hours instead.
- Components are named by page component IDs only. No snapshot carries a node, monitor or static component ID, a monitor's type, URL or hostname, a check message, a person, a revision, or anything from outside the page's components.
- Only incidents published to the page appear, open ones and those resolved within the 90 days, at most 100, and only with the page's visible components. An incident naming none of them is left out. Older ones are in the [incident history](#incident-history).
- An auto-incident's title is rendered in the snapshot's locale from the component's page name ("API is down", "API ist ausgefallen") until a person changes the title. Updates the server wrote for an auto-incident are replaced by a generic text in the locale, because check messages can name hosts.
- Maintenance shows while under way or when a window starts within 14 days, with the visible components it covers.

Colors follow the [monitoring contract](../monitoring-contract.md#component-color-and-page-status):

- A component now: the worst of its monitor (down major outage, pending degraded, up operational) and the current impact of open published incidents naming it. Maintenance overrides the monitor but not such an incident. A static component without incidents is operational.
- A bar: a published incident that touched the component that day wins with its worst impact. Otherwise uptime colors it: 100 % operational, at least 99 % degraded, at least 95 % partial outage, below that major outage, and maintenance when the day was otherwise operational. Days without heartbeats have no status, so a monitor's bars start on its first checked day. A static component is operational from the day it was created; earlier days have no status.
- Today's bar counts every heartbeat up to now. On the day of a Kuma import, Kuma's daily row is combined with the hours V1Per checked after it. This is decision 0008 item 13, still proposed; the rule lives in one function, `bar_status` in `apps/server/src/pages/snapshot.rs`.
- Day boundaries are local midnights in the page timezone with non-hour offset minutes dropped (decision 0008 item 11), for bars and for the days an incident touched.

Snapshots are cached per page, audience and locale while the page's topic stays at the same head and the heartbeat watermark has not moved, for at most a minute.

## Stream

`GET {base}/events` sends the frames of `/api/subscribe` (`subscribed`, `events`, `resync`, `expired`) for the page's one topic `page:{id}`, without their `topic` field. Subscribe from the snapshot's `cursor`. Event kinds are `page.updated` (settings, access, layout, hostnames, logo), `page.status_changed` (a shown monitor, component or maintenance), `page.incident_changed` (publication or a published incident) and `page.deleted`; each carries only `seq` and `kind`. A frame means: read `snapshot.json` again.

A wake is followed by a 250 ms pause before reading, so a burst of changes arrives as one frame. Access is checked when the stream opens (`401 unauthenticated` when closed), before every frame and every 5 seconds while idle. When it is gone the stream sends `resync` with reason `unavailable` and ends; the app then reloads the page, which shows the gate. A foreign `Origin` is refused.

## Incident history

`GET {base}/incidents.json` lists the incidents published to the page that name a component the visitor's audience sees, newest started first, with no time bound. Each item is a `PublicIncident` as in the snapshot, built by the same rules.

```ts
type PageIncidents = {
  incidents: PublicIncident[]
  next_cursor?: string // absent on the last page
}
```

- `limit`: 1 to 100, default 20.
- `cursor`: the `next_cursor` of the page before. It is opaque; a malformed one is `400 invalid_arguments` on `cursor`, as is a `limit` out of range.
- Pages are ordered by the incident's start, then its ID, so no incident appears on two pages.
- Access, `token`, `locale` and caching are those of `snapshot.json`: `401 unauthenticated` when the page is closed to the visitor.

## Badges

`{base}/badge/status.svg`, `uptime-24h.svg`, `uptime-30d.svg` and `uptime-90d.svg`, for the whole page or one `?component=<page component id>` that is not internal. They exist for public pages, and for restricted ones with `?token=<share token>`; otherwise `404`. Text is in the page's default locale. Uptime over 24 hours and 30 days uses Kuma's windows; over 90 days the page's bars. Public page badges are cached for a minute by anyone, token badges privately.

## Caching

The HTML and `snapshot.json` are `public, max-age=15` for a public page requested without cookies, `private, no-store` otherwise, with `Vary: Cookie, Accept-Language`. Access answers are `no-store`.
