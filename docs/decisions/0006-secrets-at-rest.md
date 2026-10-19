# 0006: Secrets at rest

Status: Adopted by Mike on 2026-10-18.

## Problem

Kuma stores every secret in plain text: monitor credentials, TLS keys, notification tokens, proxy passwords, status page passwords and TOTP secrets. v1per holds the same secrets for many clients.

## Constraints

One process and a data directory. Backups must be restorable. Imported credentials must keep working.

## Choice

- Seal secrets with an AEAD cipher under a key from `secrets.key` in the data directory, or from `V1PER_SECRETS_KEY` / `V1PER_SECRETS_KEY_FILE`. Reuse holocron's sealing code. The key is 32 random bytes, stored as base64; the key file also accepts raw bytes. `V1PER_SECRETS_KEY` is read from the environment only, never a flag, so it stays out of the process list. Each sealed value is bound to a context such as `monitor:<id>:password`, so a copied ciphertext does not open elsewhere.
- Sealed: monitor credentials, headers marked secret, TLS keys, notification configuration, SMTP passwords, proxy passwords, TOTP secrets.
- Page passwords are hashed with argon2, not sealed. Probe tokens, share tokens and API keys are stored as hashes.
- Secrets never appear in logs, query results, exports or the delivery log. The UI shows that a value is set and lets you replace it.
- Backups include a key reference; restore refuses to run without the matching key.

## Rejected alternatives

- Plain text like Kuma: one leaked backup exposes every client's credentials.
- An external secret store: breaks the one-process requirement.

## Consequences

Losing the key loses the secrets; the runbook says so. Key rotation is a maintenance command (11).

## Affected contracts

Milestones 02, 04, 07, 09, 10, 11.
