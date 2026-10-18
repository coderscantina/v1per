# 0005: Notifications

Status: Adopted by Mike on 2026-10-18.

## Problem

Kuma has 98 notification providers. Mike's alerts depend on email and Discord. Rebuilding all providers would dominate the first release.

## Constraints

Email and Discord must work fully. Every Kuma notification config must survive the import. Alerts must not be lost during provider outages or restarts.

## Choice

- Core providers in the first release: email (SMTP), Discord, webhook, Slack, Microsoft Teams, Telegram, ntfy, Pushover.
- Email and Discord reach parity with Kuma 2.5.0: all options, custom templates with Kuma's variables, same output for the same template.
- Channels live on nodes and are usable in their subtree. Default channels attach to new monitors in their subtree, like Kuma's `is_default`.
- Events: monitor down, monitor up, certificate expiry, probe offline. Kuma's resend interval repeats down alerts.
- Email uses the instance SMTP transport unless the channel carries its own, which imported Kuma email channels do.
- Delivery runs from a durable outbox with retries and backoff. Final failures are visible in the channel's delivery log.
- Imported channels of other providers are kept with their sealed configuration and marked inactive with a reason.

## Rejected alternatives

- Full Kuma parity: about 90 mostly mechanical adapters; better added on demand later.
- Webhook and email only: Discord would need a relay, which breaks a stated requirement.

## Consequences

The template variables Kuma exposes become a contract (recorded in 01). Adding a provider later means one adapter plus an import mapping.

## Affected contracts

`docs/contracts/kuma-notifications.md` (01), milestones 07, 10.
