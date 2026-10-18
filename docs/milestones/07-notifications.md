# Milestone 07: notifications

## Outcome

Monitor down, up, certificate expiry and probe offline events reach people by email and Discord exactly as they did from Kuma, and by the rest of the core providers.

## Dependencies and ownership

Requires 05. Monitoring owns `notifications`. Frontend owns channel screens and the channel picker in the monitor editor. Integration owns instance SMTP settings.

## Work

1. Implement channels on nodes per [0005](../decisions/0005-notifications.md): visible to the node's subtree, default channels for new monitors, sealed configuration.
2. Implement the durable outbox: one row per event and channel, retries with backoff, final failure recorded and shown, resend interval while down, suppression during maintenance.
3. Implement email over instance SMTP or a channel's own SMTP, with Kuma's options (security, from, to, cc, bcc, custom subject and body templates). Implement Discord with Kuma's options (webhook URL, display name, prefix message, templates).
4. Implement webhook, Slack, Microsoft Teams, Telegram, ntfy and Pushover with the options Kuma 2.5.0 offers for each.
5. Render Kuma's template variables, recorded in 01, so imported templates keep producing the same messages.
6. Build the UI: channels per node, test send, delivery log per channel, picker in the monitor editor.

## Acceptance criteria

- Email and Discord messages for every event match Kuma 2.5.0 output for the same templates, checked against a local SMTP sink and a Discord webhook stub.
- A provider outage delays delivery without losing it, and a restart resumes the outbox.
- No alert is sent during maintenance for monitors it covers.
- Secrets never appear in logs, results or the delivery log.

## Evidence and handoff

Message comparisons and screenshots in `docs/evidence/m07-notifications.md`. Hand the provider mapping to 10.
