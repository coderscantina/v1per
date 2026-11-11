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
| `GET {base}/incidents/{id}` | The same, opened on one incident; `404` when the visitor may not see it, see [incident pages](#incident-pages) |
| `GET {base}/incidents/{id}.json` | That incident as a `PublicIncident`; `404` likewise, `401 unauthenticated` when the page is closed to the visitor |
| `GET {base}/access/email?code=` | The same, without using the code; the page app posts `code` to `redeem` once the visitor continues |
| `GET {base}/snapshot.json` | `PageSnapshot`, or `401 unauthenticated` when the page is closed to the visitor |
| `GET {base}/incidents.json?cursor=&limit=` | `PageIncidents`, see [incident history](#incident-history) |
| `GET {base}/response-times.json?component=&range=` | `PageResponseTimes`, see [response times](#response-times) |
| `GET {base}/events?cursor=page:{id}@{seq}` | Server-sent events, see [stream](#stream) |
| `GET {base}/badge/{name}` | SVG badge, see [badges](#badges) |
| `GET {base}/logo` | The page logo, `404` without one |
| `POST {base}/access/password` | `{ password }`: `204` with the page cookie, `401 invalid_credentials` |
| `POST {base}/access/email` | `{ email, locale? }`: `202 {}` whether or not the address is allowed |
| `POST {base}/access/email/redeem` | `{ code }`: `204` with the page cookie, `401 invalid_credentials` |
| `POST {base}/subscribe`, `GET`/`POST {base}/subscription`, `POST {base}/subscription/confirm`, `POST {base}/subscription/components`, `POST {base}/unsubscribe` | Email subscriptions, see [subscriptions](#subscriptions) |
| `GET {base}/feed.atom`, `GET {base}/feed.rss`, `GET {base}/maintenance.ics` | Feeds and the maintenance calendar, see [subscriptions](#subscriptions) |

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
  slas: PageSla[] // in the page's order
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
  up?: number; down?: number; maint?: number // the checks behind uptime_90d, see the rules
  bars: { day; status?; uptime?; up?; down?; maint?; utc_approximate?; maintenance?; incidents?: IncidentId[] }[] // 90 days, oldest first
  detail?: { latency: LatencyPoint[]; probes: { name; status; latency_ms?; checked_at }[] } // monitors; see the rules
}

type PublicIncident = {
  id; title; state; impact?; started_at; resolved_at?
  components: PageComponentId[]
  updates: { state; message; created_at; impacts: { component: PageComponentId; impact }[] }[] // newest first
  postmortem?: { text: string; published_at: number } // plain text, only while resolved
}

type PublicMaintenance = { title; description; active: boolean; starts_at?; ends_at?; components: PageComponentId[] }

type PageSla = {
  name: string
  timezone: string // IANA zone of the periods
  period: 'month' | 'quarter' | 'year'
  combine: 'each' | 'any_down'
  current?: PageSlaPeriod // the running period, absent before the contract starts
  history: PageSlaPeriod[] // up to 12 frozen periods, newest first
}

type SlaOutcome = 'met' | 'missed' | 'on_track' | 'at_risk' | 'no_data' // at_risk: running only

type PageSlaPeriod = {
  start: string // YYYY-MM-DD, first day in the SLA's timezone
  starts_at: number
  ends_at: number // first instant after the period
  outcome: SlaOutcome // the worst commitment: missed, at_risk, on_track, met; no_data when none has data
  availability?: { target: number; outcome: SlaOutcome; actual?: number; budget_used?: number } // percent; budget share, 1 is all
  latency?: { target_ms: number; outcome: SlaOutcome; actual_ms?: number } // highest monitor average
  response?: PageSlaClock
  resolution?: PageSlaClock
  monitors?: { component: PageComponentId; availability?; availability_outcome?; latency_ms?; latency_outcome? }[] // internal only
}

type PageSlaClock = {
  targets: { major_outage?: number; partial_outage?: number; degraded?: number } // business minutes
  outcome: SlaOutcome
  counted: number // incidents that met or missed the target
  met: number
  within_target?: number // met / counted, absent while none is counted
  average_ms?: number // business milliseconds
}
```

Rules:

- The `public` audience sees only components not marked `internal`; groups left empty are dropped. The `internal` audience sees every component and gets `detail` on monitor components: hourly response times over the last 24 hours and each assigned probe's status. While a monitor has fewer than two hourly points, as a new one does, its response times are per minute over the last 2 hours instead.
- On a page with the `response_times` setting, the `public` audience gets `detail` on its monitor components too, with the same `latency` and always empty `probes`. Probe names and per-probe results stay internal.
- Components are named by page component IDs only. No snapshot carries a node, monitor or static component ID, a monitor's type, URL or hostname, a check message, a person, a revision, or anything from outside the page's components.
- Only incidents published to the page appear, open ones and those resolved within the 90 days, at most 100, and only with the page's visible components. An incident naming none of them is left out. Older ones are in the [incident history](#incident-history).
- An auto-incident's title is rendered in the snapshot's locale from the component's page name ("API is down", "API ist ausgefallen") until a person changes the title. Updates the server wrote for an auto-incident are replaced by a generic text in the locale, because check messages can name hosts.
- Maintenance shows while under way or when a window starts within 14 days, with the visible components it covers.
- `up`, `down` and `maint` are the monitor-level checks behind an uptime figure, on a bar for its day and on a component summed over its bars. Uptime is `up / (up + down)`. `down` includes checks still retrying, as uptime counts them; `maint` are checks under maintenance, which uptime leaves out. A count is absent while zero. The page app explains every uptime figure from these counts and derives nothing else from them.
- `postmortem` is what the team set with `incidents.setPostmortem`. It appears only while the incident is resolved; reopening hides it and keeps the text. The page app renders it as plain text with its line breaks, never as HTML or Markdown.
- `slas` lists the SLAs selected with `pages.updateSlas`, per the [monitoring contract](../monitoring-contract.md#slas). Each period carries the targets it was computed with. Counts and averages include incidents that were never published, drafts too, but no incident, its title or its ID appears. No SLA ID, covered monitor list, channel or person appears either.
- The `internal` audience also gets `monitors` per period: the covered monitors the page shows, named by their page component. A covered monitor not on the page is only in the aggregates.
- `at_risk` is the server's call on a running period: 75 % of the downtime budget used and not exceeded, or an average latency above its target. Clients show it and derive nothing from the figures. A period in which no covered monitor has data and no incident started is `no_data`, for the response and resolution clocks too.
- A closed period appears in `history` once it is frozen, an hour after it closes. The running period is computed at most once a minute per SLA, so its numbers can lag that long behind the snapshot.

Colors follow the [monitoring contract](../monitoring-contract.md#component-color-and-page-status):

- A component now: the worst of its monitor (down major outage, pending degraded, up operational) and the current impact of open published incidents naming it. Maintenance overrides the monitor but not such an incident. A static component without incidents is operational.
- A bar: a published incident that touched the component that day wins with its worst impact. Otherwise uptime colors it: 100 % operational, at least 99 % degraded, at least 95 % partial outage, below that major outage, and maintenance when the day was otherwise operational. Days without heartbeats have no status, so a monitor's bars start on its first checked day. A static component is operational from the day it was created; earlier days have no status.
- Today's bar counts every heartbeat up to now. On the day of a Kuma import, Kuma's daily row is combined with the hours V1Per checked after it. This is decision 0008 item 13, still proposed; the rule lives in one function, `bar_status` in `apps/server/src/pages/snapshot.rs`.
- Day boundaries are local midnights in the page timezone with non-hour offset minutes dropped (decision 0008 item 11), for bars and for the days an incident touched.

Snapshots are cached per page, audience and locale while the page's topic stays at the same head and the heartbeat watermark has not moved, for at most a minute.

## Stream

`GET {base}/events` sends the frames of `/api/subscribe` (`subscribed`, `events`, `resync`, `expired`) for the page's one topic `page:{id}`, without their `topic` field. Subscribe from the snapshot's `cursor`. Event kinds are `page.updated` (settings, access, layout, hostnames, logo, SLA selection), `page.status_changed` (a shown monitor, component or maintenance, a shown SLA's terms, or one of its periods frozen or recomputed), `page.incident_changed` (publication or a published incident) and `page.deleted`; each carries only `seq` and `kind`. A frame means: read `snapshot.json` again.

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

## Incident pages

`GET {base}/incidents/{id}` serves the page app on one incident: summary, postmortem and the full update timeline. `GET {base}/incidents/{id}.json` answers the `PublicIncident`, which the app reads for an incident the snapshot no longer lists. Both use the builder behind `incidents.json`, so the visibility rules are the same: the incident must be published to this page and name a component the visitor's audience sees. Anything else, a malformed ID included, is `404`. On a page closed to the visitor the HTML route serves the gate and the JSON route answers `401 unauthenticated`, whether or not the incident exists.

## Link previews

The HTML of a page the visitor may read carries `<meta name="description">`, Open Graph (`og:type`, `og:title`, `og:description`) and Twitter card tags, in the page's default locale:

- On the page and its views: the page title, and the overall status in words before the page description.
- On an incident page: the incident title, then its state, its impact in words (the worst it reached once resolved) and its latest update text.

Descriptions are cut to 200 characters. `og:url` and `<link rel="canonical">` appear when the page's origin is known: a claimed hostname, or the instance's public URL. They never carry a share token. `og:image` is the page logo when it is a raster image; an SVG logo or none leaves the image out. Every value is escaped for an HTML attribute.

The gate HTML carries none of these tags, so a restricted page's preview names neither its incidents nor its status.

## Response times

`GET {base}/response-times.json?component=<page component id>&range=24h|7d|30d|90d` answers one monitor component's response times over a range ending now, for the chart under the component. The snapshot's `detail.latency` stays, so the first paint needs no extra request; the page app reads this route when the block scrolls into view and when the visitor picks a range.

```ts
type PageResponseTimes = {
  range: '24h' | '7d' | '30d' | '90d'
  from: number // the span the points cover
  to: number
  step: number // bucket width in milliseconds; a longer step between two points is a gap
  overall: ResponseSeries // every check of the monitor
  series: ResponseSeries[] // per probe or per probe location, see below
  marks: { kind: 'incident' | 'maintenance'; incident?: IncidentId; impact?: Impact; title: string; from: number; to?: number }[] // oldest first
}

type ResponseSeries = {
  label?: string // absent on overall
  country?: string // ISO 3166-1 alpha-2
  points: LatencyPoint[] // buckets with up checks, oldest first
  avg_ms?: number; p50_ms?: number; p95_ms?: number; p99_ms?: number // over the up checks in the range
}
```

- Access, `token`, `locale` and the `Cache-Control` header are those of `snapshot.json`: `401 unauthenticated` when the page is closed to the visitor.
- `404 not_found` when the audience gets no response times for the component: the `public` audience on a page without `response_times`, a component the audience does not see, or one that is not a monitor.
- A missing or unknown `range`, or a missing `component`, is `invalid_arguments` with status `422`, as the API maps that code everywhere.
- Steps: hourly for `24h` and `7d`, 4 hours for `30d`, daily for `90d`, so a series has at most 180 points. A monitor with fewer than two hourly points gets its last 2 hours per minute over `24h`, as in the snapshot; `from` and `step` say so.
- `series` for the `internal` audience: one per assigned probe, labelled with the probe's name and its location when set.
- `series` for the `public` audience: empty unless the page has both `response_times` and `probe_locations`. Then one per location, labelled "City, CC" or the country alone. Probes with the same city and country are merged. A probe without a country has no series and still counts in `overall`. No probe name, provider, platform or ID reaches the public audience.
- Points come from rollups. The average and percentiles come from the raw heartbeats of the range: `overall` from every probe's up checks, a series from its probes'. Raw heartbeats are kept 90 days and each closed UTC day is rewritten once, sorted by monitor, so the read touches one monitor's rows ([decision 0002](../decisions/0002-storage-split-and-retention.md)).
- `marks` are the published incidents that named the component and the maintenance windows that covered its monitor within the range, built by the snapshot's rules: only incidents published to the page, titles as the snapshot shows them, `impact` the worst on this component.
- Answers are cached per page, audience, locale, component and range while the page's topic stays at the same head, for at most a minute. Unlike the snapshot cache this one does not follow new heartbeats, so the newest point can lag a minute.

## Subscriptions

Everything here exists while the page's `subscriptions` setting is on; otherwise each route answers `404` and the snapshot has no `subscriptions` field. Wire types: `PageSubscriptions`, `PageSubscribeBody`, `PageSubscriptionToken`, `PageSubscriptionComponents`, `PageSubscription`, `PageNotification`.

```ts
// PageSnapshot.subscriptions, absent with the setting off
type PageSubscriptions = {
  email: boolean // the instance has a mail server, so POST {base}/subscribe sends links
  feeds: boolean // the page is public, so the feeds and the calendar open without a token
  components: PageComponentId[] // what a subscription may follow: components not marked internal
}
```

### Email

| Method and path | Answer |
| --- | --- |
| `POST {base}/subscribe` | `{ email, components?, locale? }`: `202 {}` whether or not a mail was sent |
| `GET {base}/subscription?token=` | The page app, which shows the subscription. Changes nothing |
| `POST {base}/subscription` | `{ token }`: `PageSubscription`, `401 invalid_credentials` for an unknown or expired token |
| `POST {base}/subscription/confirm` | `{ token }`: confirms, answers `PageSubscription` |
| `POST {base}/subscription/components` | `{ token, components }`: replaces what it follows, answers `PageSubscription` |
| `POST {base}/unsubscribe` | `{ token }`, or `?token=` from a mail client: deletes the subscriber, `204` either way |

- Double opt-in. `subscribe` stores the address unconfirmed and mails a link to `{origin}{base}/subscription?token=<secret>`, valid 24 hours. Opening the link confirms nothing: the page app shows the subscription and posts the token to `confirm` only after the visitor clicks, so a mail scanner that follows links confirms nobody. Unconfirmed rows are deleted by the hourly retention pass.
- The visitor must be able to open the page to subscribe. On a restricted page the address must also be in the page's `emails` or `domains`, checked at `subscribe`, at `confirm` and at every send. A subscriber whose address is no longer allowed gets nothing.
- `components` empty or absent follows the whole page. Otherwise only incidents and maintenance naming one of them notify. A component that is internal or not on the page is `invalid_value` on `components`.
- One secret per subscriber confirms, opens the settings and unsubscribes. It is derived from the instance key, the page, the address and the time the subscription started, and stored only as its SHA-256. The subscription routes open with the token alone, also on a restricted page, where the HTML then carries the gate boot and the app shows the subscription view.
- Every notification mail carries the manage link, an unsubscribe link (`...&action=unsubscribe`, which also waits for a click) and `List-Unsubscribe` with `List-Unsubscribe-Post: List-Unsubscribe=One-Click` naming `POST {base}/unsubscribe?token=`. That one-click request comes from a mail client without an `Origin`, so `unsubscribe` accepts a missing `Origin` and refuses a foreign one.
- The `POST` routes share the access routes' rules: the page's own `Origin`, a JSON body of at most 4 KiB, 10 attempts per minute per client address and 3 mails per minute per recipient.
- Stored per subscriber: address, locale, component selection, when it started and when it was confirmed, and the token hash. No client address. A page holds at most 5,000 subscribers; past that `subscribe` still answers `202` and sends nothing.
- Links need an origin: the claimed hostname the request came to, else the instance's public URL. Without one no confirmation is sent, as for sign-in links.

### Feeds and calendar

`{base}/feed.atom` and `{base}/feed.rss` list the 50 newest published incidents with their updates, built by the code behind `incidents.json` for the `public` audience. `{base}/maintenance.ics` is an iCalendar feed of the maintenance the public snapshot shows. Like badges they open on public pages, and on restricted ones with `?token=<share token>`; otherwise `404`. The HTML of a public page links both feeds with `<link rel="alternate">`.

### Webhooks

Page managers add webhook URLs with `pages.addWebhook`; visitors cannot, so nobody anonymous makes the server call an address. A webhook gets a `PageNotification` as a JSON `POST`, in the page's default locale:

```ts
type PageNotification = {
  event: 'incident_published' | 'incident_updated' | 'incident_resolved' | 'maintenance_scheduled' | 'maintenance_started'
  page: { slug; title; url? }
  incident?: PublicIncident // as the public snapshot shows it
  maintenance?: PublicMaintenance
  components: { id: PageComponentId; name }[]
  sent_at: number
}
```

The URL is stored sealed and never answered; lists show its host. A page holds at most 20 webhooks.

### What notifies

- An incident being published to the page, each later update on a published incident, and its resolution, including the automatic resolution of an auto-incident. The notice is written in the transaction of the change, with the incident as the public page shows it then: only public components, auto-incident texts in their generic localized form.
- A maintenance window when it first shows on the page (14 days ahead) and when it starts. The delivery worker looks once a minute, so these can be up to a minute late. A window first seen under way is announced once, as started.
- Unpublishing notifies nobody, and deliveries still queued for an incident that is no longer published are dropped. So are deliveries for a page that turned subscriptions off.
- Deliveries run in the background, per subscriber in order, and retry like the notification outbox: after 10 seconds, doubling up to 15 minutes, for 24 hours. A permanent SMTP error or a non-retryable HTTP answer drops the delivery.

Admin operations: `pages.subscribers` lists confirmed addresses and webhooks, `pages.removeSubscriber`, `pages.addWebhook` and `pages.removeWebhook` change them, all with `pages.manage`. They record `page.subscribers_changed` on the node's `pages:<id>` topic, as do a visitor's confirm, selection change and unsubscribe.

## Badges

`{base}/badge/status.svg`, `uptime-24h.svg`, `uptime-30d.svg` and `uptime-90d.svg`, for the whole page or one `?component=<page component id>` that is not internal. They exist for public pages, and for restricted ones with `?token=<share token>`; otherwise `404`. Text is in the page's default locale. Uptime over 24 hours and 30 days uses Kuma's windows; over 90 days the page's bars. Public page badges are cached for a minute by anyone, token badges privately.

## Caching

The HTML and `snapshot.json` are `public, max-age=15` for a public page requested without cookies, `private, no-store` otherwise, with `Vary: Cookie, Accept-Language`. Access answers are `no-store`.
