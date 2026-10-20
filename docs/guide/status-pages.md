# Status pages

A status page shows the state of monitors and static components, with incidents and maintenance, to the public, to a client or to your team. Pages belong to a node and can show anything from that node and the nodes below it.

Create a page from the node's Status pages screen. Creating and editing pages needs the Admin role on the node. The editor has five tabs: Settings, Components, Access, Domains and Preview.

## Address

Every page is reachable at `/s/<slug>` on the instance, for example `https://status.example.com/s/acme`. The slug is called Address in the editor: 1 to 64 lowercase letters, digits and dashes, unique on the instance.

## Components

In the Components tab, pick monitors and static components from the page's node and the nodes below it, and sort them into groups.

- A group has an optional heading and can start open or closed.
- A component can have its own shown name.
- **Team only** hides a component from everyone but people with a role on the node.
- A page holds up to 50 groups and 200 components. Each monitor or component appears once per page.

Static components have no monitor. Their color comes from incidents and maintenance only.

## Access modes

New pages are for your team until you open them up. Set who gets in on the Access tab.

| Mode | How a visitor gets in |
| --- | --- |
| Public | Anyone with the address opens the page |
| Login | People with a role on the page's node or a node above it sign in to the instance. Always on |
| Password | The visitor types the page password, 1 to 256 characters |
| Sign-in link | The visitor enters their email and gets a link that opens the page |
| Share link | The visitor opens a secret link you gave them |

While a page is public, the password, sign-in link and share link settings are ignored. A restricted page with none of them set is team only.

**Login.** Signed-in team members see the team view: team-only components, response times and the status per probe. This works at `/s/<slug>` only, not on custom domains. On a custom domain the gate's sign-in link takes team members to the page on the instance address.

**Password.** Visitors who know it stay in for 30 days. Changing or removing the password signs out everyone who got in with it.

**Sign-in link.** List allowed addresses or whole domains, one per line, up to 200 together. A listed visitor asks for a link on the page, gets it by email in the page's language, and opens it within 15 minutes. Each link works once. Sign-in links need the instance mail server, and on the instance address they also need the [public URL](configuration.md#public-url). The page answers the same way for every address, so nobody can test which ones are allowed. Removing an address from the list ends its sessions.

**Share link.** In the Share links panel, create a link with a name for who it is for and an optional expiry date. The link, `<page address>/t/<secret>`, is shown once. A page holds up to 50 share links. Revoking one ends it at once, also for visitors who already opened the page with it.

Visitors stay in for 30 days, or until a share link expires if that comes first. Password tries and link requests are limited to 10 a minute per client address, and to 3 links a minute per recipient.

## Custom domains

A page can also answer on its own domains, up to 10 per page.

1. In the Domains tab, add the hostname, for example `status.acme.example`.
2. Point the domain's DNS at your reverse proxy.
3. Add the domain to the proxy so it gets a certificate. See [Reverse proxy and custom domains](install.md#reverse-proxy-and-custom-domains).

V1Per picks the page by the `Host` header. A custom domain serves only its page at the root path. The admin app and the API are not reachable on it. A hostname can belong to one page only, and the instance's own public host cannot be claimed.

## Branding

In the Settings tab:

- **Title**, up to 100 characters, a **description** up to 2,000 and a **footer** up to 1,000. Both show as plain text.
- **Logo**: PNG, JPEG, GIF, WebP or SVG, up to 512 KB. One logo for light and dark themes.
- **Theme**: follow the device, light or dark.
- **Accent color**: indigo, blue, violet, rose, orange, amber, emerald or cyan.
- **Timezone** of the daily bars.

## Languages

Pages come in English and German. Set a default language and which other languages the page offers.

A visitor gets the language from `?locale=en` or `?locale=de` in the address, else from their browser's language, else the page default. With more than one language offered, the page footer shows a switch. Badges always use the default language.

## Live updates

Open pages update by themselves through server-sent events when a monitor changes, an incident is published or updated, or the page settings change. If a visitor loses access, for example because a share link was revoked, the page reloads into the access gate.

## Badges

Every page has SVG badges for READMEs and dashboards:

| Badge | Shows |
| --- | --- |
| `<page address>/badge/status.svg` | The page status, such as "operational" |
| `<page address>/badge/uptime-24h.svg` | Uptime over 24 hours |
| `<page address>/badge/uptime-30d.svg` | Uptime over 30 days |
| `<page address>/badge/uptime-90d.svg` | Uptime over 90 days |

Add `?component=<component id>` for one component instead of the whole page. Team-only components have no badge. Badges of public pages work for anyone. For a restricted page, add `?token=<share link secret>`.

## Incidents on pages

An incident has updates with a state (investigating, identified, monitoring, resolved) and an impact per component (operational, degraded, partial outage, major outage). The latest update that names a component sets its impact. Resolving sets every affected component back to operational.

Members and up create and update incidents. A manual incident is published to the pages that show its components. Publishing to a page also needs the Member role on the page's node.

The page has three views: Status, Maintenance and Incidents. Status shows active incidents first, each with its update timeline. Incidents lists the resolved ones, 10 at a time. Maintenance lists the windows under way and those starting within 14 days.

**Auto-incidents** open when a monitor on the page goes down and resolve when it recovers. Choose per page in the Settings tab:

- **Keep as draft**: they stay internal until someone publishes them.
- **Publish automatically**: they show once they stayed open for the grace period, 0 to 1,440 minutes. An outage that ends sooner never shows.

Auto-incidents show a generic text, never the check's error message.

## Maintenance on pages

Members and up schedule maintenance: one-off, recurring by interval, weekday or day of month, by cron expression, or started by hand, each in a timezone. A page shows maintenance while it runs and from 14 days before it starts. Affected components show as under maintenance unless a published incident says worse.

## Page status and colors

Each component takes the worst of its monitor status (down is a major outage, pending is degraded), the impact of published incidents that name it, and maintenance. The page status is the worst component status. Each daily bar shows the worst state of that day and its uptime.

## Nodes, roles and invites

Everything in V1Per sits in a tree of nodes, for example your agency at the root, then a node per client, and projects below that.

| Role | Can |
| --- | --- |
| Viewer | Read the node and everything below it, and open its status pages |
| Member | Also manage monitors, incidents and maintenance |
| Admin | Also manage status pages, channels, probes, node settings and members |
| Owner | Also move and delete the node |

A role on a node applies to that node and everything below it. Roles only add up: your role on a node is the highest one you hold there or above. People see nothing outside the nodes they hold a role on, not even the names of other clients or the parent node. The instance admin owns the root and so everything.

To invite someone:

1. On the node's Members page, choose invite, enter their email, pick a role and a validity of 1 to 30 days (7 by default).
2. Copy the link that is shown once and send it to them yourself. V1Per sends no invitation email.
3. They open the link and create an account with the invited email, or accept with the account they are signed in with.

You can invite only with roles below your own. Owners can also invite owners. Pending invitations can be revoked.

A node can require two-factor authentication in its settings. Everyone with a role on it or below must then set up a second factor at their next request.
