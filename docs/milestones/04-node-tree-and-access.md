# Milestone 04: node tree and access

## Outcome

An instance admin builds a tree like `client services > autohaus > schmidt`, invites people at any node, and each person sees and changes exactly what their inherited grants allow.

## Dependencies and ownership

Requires 03. Integration owns `nodes` and `identity`. Frontend owns the tree navigation, member and settings screens.

## Work

1. Implement nodes per [0001](../decisions/0001-node-tree-and-access.md): create, rename, move, delete (only when empty, or with explicit cascade), depth limit, ordering.
2. Implement grants and `NodeScope`: effective role is the highest grant on the node or an ancestor; operations take a scope and never reveal nodes outside it. Write the capability matrix in `docs/contracts/capability-matrix.md`.
3. Copy holocron's invitations, TOTP 2FA with recovery codes, node-level 2FA requirement and scoped API keys, adapted to nodes.
4. Add the instance admin flag and instance settings (public URL, SMTP placeholder until 07).
5. Build the UI: tree in the sidebar context selector, node overview, members and invitations, API keys, account security.

## Acceptance criteria

- The tree from the briefing is built in a test, and each grant in it gives exactly the expected visible nodes and capabilities.
- A user cannot raise their own role, outrank-check follows holocron's rank rule, and moving a node requires rights on both sides.
- Refusals do not disclose whether an inaccessible node exists.
- 2FA, recovery codes and API key scopes work as in holocron.

## Evidence and handoff

Permission matrix test output and screenshots in `docs/evidence/m04-node-tree-and-access.md`. Hand `NodeScope` to 05 through 09.
