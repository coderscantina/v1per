# 0001: Node tree and access

Status: Adopted by Mike on 2026-10-18.

## Problem

One instance hosts Mike's own operations and products and several clients. Some clients should see a whole subtree (autohaus), others a single project (schmidt). A fixed team and project hierarchy cannot express that.

## Constraints

Self-hosted, many teams on one instance. Holocron's roles, rank rule, 2FA and API keys are the starting point. Clients must not learn about each other.

## Choice

- One tree of uniform nodes of any depth. Example:
  - `operations > scaleway | aws`
  - `products > b10cks | ravn`
  - `client services > kaessbohrer > website | landing page`
  - `client services > autohaus > schmidt | motormobile`
- Any node can hold monitors, status pages, notification channels, probe agents, incidents, maintenance windows and grants. Nodes differ only by position.
- A grant gives a user one role (Owner, Admin, Member, Viewer) on a node. It applies to the whole subtree. The effective role is the highest grant on the node or any ancestor. There are no deny rules or lowering overrides.
- A client is a node with invited people. In the example, kaessbohrer's contacts get a grant on `kaessbohrer`; autohaus contacts get a grant on `autohaus` and so see schmidt and motormobile; schmidt's contact gets a grant on `schmidt` only.
- One instance admin flag: instance settings, SMTP, the root node, shared probes, backups. There are no personal workspaces.
- Channels and probes on a node are usable by everything in its subtree.
- An opaque `NodeScope` from authorization is passed to every domain operation. Refusals do not disclose whether a node exists.
- Proposed role meaning (finalized in the capability matrix in 04): Viewer reads monitors, pages and incidents; Member also manages monitors, incidents and maintenance; Admin also manages pages, channels, probes, child nodes and invites up to Admin; Owner also manages Owners and moves or deletes the node.

## Rejected alternatives

- Fixed team, client and project levels: cannot express "client access to autohaus, project access to schmidt".
- A separate client entity: adds a second membership system for the same need.
- Inheritance with overrides or deny rules: harder to reason about and to audit; not needed for the stated cases.
- Holocron's personal workspaces: no use case on a monitoring instance.

## Consequences

- Moving a node changes who sees it. The UI shows the change in visible members before confirming.
- A monitor can only use channels and probes from its own node or ancestors; moving it can invalidate those references, so a move checks and reports them.
- Status pages can only pick components from their own subtree ([0004](0004-status-pages-and-incidents.md)).

## Affected contracts

Capability matrix (04), protocol scope types, milestones 04 to 10.
