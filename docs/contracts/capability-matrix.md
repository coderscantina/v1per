# Capability matrix

Milestone 04 finalizes the role meaning proposed in [0001](../decisions/0001-node-tree-and-access.md). The code owns the same table in `crates/protocol/src/access.rs` and exports it to `apps/web/src/generated/access.ts`. The fixture `apps/server/tests/fixtures/node-tree.json` pins it, and `apps/server/tests/access.rs` checks it against the API on the briefing's example tree.

## Rules

- A grant gives one role on a node. It applies to the node and its whole subtree.
- The effective role on a node is the highest grant on the node or any ancestor. Grants only add; there are no deny rules.
- The instance admin acts as owner of the root, so as owner everywhere.
- Every node-scoped operation resolves a `NodeScope` first. A node the caller cannot read answers `not_found`, byte for byte like a node that does not exist. A readable node without the needed capability answers `forbidden`.
- Checks run in a fixed order: scope (`not_found`), capability (`forbidden`), rank (`forbidden`), fresh login (`reauthentication_required`), expected revision (`stale_revision`). A refusal never says more than the caller may know.

## Roles and capabilities

| Capability           | Allows                                                                 | Viewer | Member | Admin | Owner |
| -------------------- | ---------------------------------------------------------------------- | :----: | :----: | :---: | :---: |
| `node.read`          | See the node, its subtree, members, monitors, status pages, incidents  |   x    |   x    |   x   |   x   |
| `monitors.manage`    | Create, change, pause and delete monitors and see push tokens          |        |   x    |   x   |   x   |
| `incidents.manage`   | Open, update, resolve and delete incidents (milestone 08)              |        |   x    |   x   |   x   |
| `maintenance.manage` | Plan, pause and delete maintenance windows (milestone 08)              |        |   x    |   x   |   x   |
| `pages.manage`       | Create and change status pages, their access and share tokens, and static components |        |        |   x   |   x   |
| `channels.manage`    | Create and change notification channels (milestone 07)                 |        |        |   x   |   x   |
| `probes.manage`      | Register and revoke probe agents                                       |        |        |   x   |   x   |
| `node.write`         | Rename the node, create and order its children                         |        |        |   x   |   x   |
| `members.manage`     | Invite, change and remove grants under the rank rule, require 2FA      |        |        |   x   |   x   |
| `node.move`          | Move the node to another parent                                        |        |        |       |   x   |
| `node.delete`        | Delete the node and, with confirmation, its subtree                    |        |        |       |   x   |

Capabilities without an operation yet are the contract for the milestones named in the table.

## Operations

| Operation                          | Required                                                                                  | Roles                          |
| ---------------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------ |
| `nodes.tree`                       | Signed in; shows only what the caller reads                                               | any                            |
| `nodes.get`                        | `node.read`                                                                               | viewer and up                  |
| `nodes.create`                     | `node.write` on the parent                                                                | admin and up                   |
| `nodes.rename`                     | `node.write`                                                                              | admin and up                   |
| `nodes.reorder`                    | `node.write` on the parent                                                                | admin and up on the parent     |
| `nodes.move`                       | `node.move` on the node, `node.write` on the current and the new parent                  | owner of the node, admin on both parents |
| `nodes.delete`                     | `node.delete`, fresh login, descendant count when not empty                               | owner                          |
| `nodes.requireTwoFactor`           | `members.manage`, fresh login                                                             | admin and up                   |
| `members.list`                     | `node.read`                                                                               | viewer and up                  |
| `members.invitations`              | `members.manage`                                                                          | admin and up                   |
| `members.invite`                   | `members.manage` and the rank rule for the invited role                                   | admin and up                   |
| `members.revokeInvitation`         | `members.manage` on the invitation's node                                                 | admin and up                   |
| `members.updateRole`               | `members.manage`, the rank rule, fresh login when raising to admin or owner               | admin and up                   |
| `members.remove`                   | Own grant: `node.read`. Someone else's: `members.manage` and the rank rule                | anyone for themselves, admin and up for others |
| `invitations.preview`, `invitations.accept` | A pending invitation code                                                        | anyone with the link           |
| `apiKeys.create`                   | Fresh login; every capability held on the key's node and key-grantable                  | viewer and up, within their role |
| `apiKeys.revoke`, `apiKeys.list`   | The caller's own keys                                                                     | any                            |
| `instance.settings`, `instance.updateSettings` | The instance admin flag                                                       | instance admin                 |
| `monitors.list`, `monitors.get`, `monitors.heartbeats`, `monitors.uptime`, `monitors.latency`, `monitors.dayBars` | `node.read` on the monitor's node; `monitors.get` shows the push token only with `monitors.manage` | viewer and up |
| `monitors.create`, `monitors.update`, `monitors.pause`, `monitors.resume`, `monitors.setManualStatus`, `monitors.delete` | `monitors.manage` on the monitor's node; a `group` must sit on that node or an ancestor the caller reads; `probes` must be registered on that node or an ancestor, readable or not | member and up |
| `probes.list`, `probes.available`      | `node.read`; `probes.available` also names probes on ancestors the caller cannot read, without their node | viewer and up |
| `probes.create`                        | `probes.manage` on the node; instance-wide probes go on the root                          | admin and up                   |
| `probes.revoke`                        | `probes.manage` on the probe's node; the built-in probe cannot be revoked                 | admin and up                   |
| `channels.list`, `channels.get`, `channels.deliveries`, `channels.delivery` | `channels.manage` on the channel's node                    | admin and up                   |
| `channels.monitorDeliveries`           | `node.read` on the monitor's node; lists deliveries through channels on that node and on ancestors the caller can read, with the provider's `error` and `detail` only where the caller has `channels.manage` | viewer and up |
| `channels.available`                   | `node.read`; also names channels on ancestors the caller cannot read, without their node | viewer and up |
| `channels.create`, `channels.update`, `channels.delete`, `channels.test` | `channels.manage` on the node; a test's `channel` must sit on that node | admin and up |
| `monitors.create`, `monitors.update` with `channels` | Each channel on the monitor's node or an ancestor, readable or not            | member and up                  |
| `instance.updateSmtp`, `instance.testSmtp` | The instance admin flag; `instance.testSmtp` five times a minute per person          | instance admin                 |
| `imports.list`, `imports.get`, `imports.inventory` | The instance admin flag; `imports.get` and `imports.inventory` answer `not_found` for a node without an import | instance admin |
| `incidents.list`, `incidents.get`      | `node.read`                                                                               | viewer and up                  |
| `incidents.create`, `incidents.addUpdate`, `incidents.update`, `incidents.delete` | `incidents.manage` on the incident's node; targets on that node or below | member and up |
| `maintenance.list`, `maintenance.get`  | `node.read`                                                                               | viewer and up                  |
| `maintenance.preview`                  | Signed in                                                                                 | any                            |
| `maintenance.create`, `maintenance.update`, `maintenance.pause`, `maintenance.resume`, `maintenance.delete` | `maintenance.manage` on the maintenance's node; monitors on that node or below | member and up |
| `components.list`                      | `node.read`                                                                               | viewer and up                  |
| `components.create`, `components.update`, `components.delete` | `pages.manage` on the component's node                              | admin and up                   |
| `pages.list`, `pages.get`, `pages.preview` | `node.read` on the page's node                                                    | viewer and up                  |
| `pages.create`, `pages.update`, `pages.updateAccess`, `pages.updateLayout`, `pages.claimHostname`, `pages.releaseHostname`, `pages.removeLogo`, `pages.delete`, `pages.createShareToken`, `pages.revokeShareToken`, logo upload | `pages.manage` on the page's node; layout targets on that node or below | admin and up |
| `incidents.publications`               | `node.read` on the incident's node; lists only pages on nodes the caller reads            | viewer and up                  |
| `incidents.publish`, `incidents.unpublish` | `incidents.manage` on the incident's node and on the page's node; publishing needs the page to show one of the incident's targets | member and up |
| `incidents.create` (publication)       | Publishes to the pages showing a target on whose node the author has `incidents.manage`  | member and up                  |

`account.*`, `auth.*` and `twoFactor.*` act on the caller's own account and take no node scope. `auth.reauthenticate` gives a fresh login for 10 minutes. `twoFactor.disable`, `twoFactor.regenerateRecoveryCodes` and replacing an enrolled factor need one too.

## Rank rule

Taken from holocron's R-RANK and applied to effective roles on the node:

- An actor changes or removes only a grant whose holder's effective role is below the actor's own, and grants only roles below its own.
- Owners may also make owners and change other owners.
- Nobody changes their own grant, so nobody raises their own role. Anyone may remove their own grant; grants on ancestors stay.
- No node is left without an owner: the instance admin owns the root.

## API keys

A key reaches one node and its subtree, with a set of capabilities. On every request it acts as its owner with the owner's current role there, never more: the scope is the owner's capabilities on the node intersected with the key's. A key cannot hold `members.manage`, `node.move` or `node.delete`: who may access a node and whether it exists stay with people. Keys call only operations marked `key` in the registry (`nodes.tree`, `nodes.get`, `nodes.create`, `nodes.rename`, `nodes.reorder`, every `monitors.*`, `incidents.*` and `maintenance.*` operation, `probes.list`, `probes.available`, `channels.available`, `components.list`, `pages.list`, `pages.get` and `pages.preview`); anything else is `operation_not_granted`.

## Status page visitors

Public page routes take no admin session or API key and are described in [status-page-api.md](status-page-api.md). A signed-in person whose scope resolves on the page's node, so Viewer and up there or on an ancestor, reads any page with the internal snapshot. Everyone else needs a public page, the page password, a sign-in link or a share token, and gets the public snapshot.

## 2FA requirement

`nodes.requireTwoFactor` marks a node. Everyone holding a grant on that node or below it must use 2FA; the instance admin counts as holding the root. Like grants, the requirement flows down the tree: a requirement on a node does not reach people whose grants sit above it or in another branch. Without a confirmed factor their session becomes enrollment-only at the next request and answers `second_factor_enrollment_required` for everything except enrolling, `account.get` and `auth.logout`. API keys keep working.

## Visible tree

`nodes.tree` returns every node the caller reads and, above them, the ancestors needed to place them as placeholders: ID and position, no name, no role, no capabilities. `nodes.get` shows the same for a node's path. Names of ancestors above the caller's highest grant stay hidden, so two clients under one parent learn neither each other's nor the parent's name. Node IDs are not secret; every operation on a placeholder answers `not_found`.
