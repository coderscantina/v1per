# Security policy

v1per is a self-hosted monitoring and status page server. One instance holds monitor targets, credentials for notification channels, and access rules for many teams and clients. We treat authentication, authorization, node isolation, probe trust, and release integrity as security-sensitive.

The project is pre-alpha and has no releases yet. Reports are still welcome.

## Supported versions

Security fixes are guaranteed for the latest release. Older releases do not receive backported patches.

| Release | Status        |
| ------- | ------------- |
| Latest  | Supported     |
| Older   | Not supported |

## Reporting a vulnerability

**Do not open a public GitHub issue for an unpatched vulnerability.**

Report suspected security issues through one of these channels, in order of preference:

1. **GitHub Security Advisories**, using private reporting:
   [Submit a security advisory](../../security/advisories/new)

2. **Email** to `security@coderscantina.com`
   Subject line: `v1per security report`

Please include as much of the following as possible:

- v1per version (release tag or commit SHA)
- Environment: Docker or source build, OS, whether remote probes are in use
- Affected component (API, auth, status page access, probe protocol, notifications, import, etc.)
- Reproduction steps or proof of concept
- Impact summary: what data or access is at risk and for whom
- Your estimated CVSS 3.1 score (optional, we may adjust)

You don't need to request a CVE. We handle that as part of the advisory process.

## Response expectations

- Acknowledgement within **72 hours**.
- Status update within **14 calendar days** if a fix is not yet available.
- We coordinate public disclosure after a fix or mitigation is ready.
- Credit is given to reporters in the advisory and release notes, with your consent.

## Disclosure policy

Once a fix is prepared:

1. A patch release is issued.
2. Release notes say that a security issue was addressed.
3. Detailed disclosure is delayed by 2 to 8 weeks depending on severity, so operators can upgrade.
4. The GitHub Security Advisory and CVE (if applicable) are published after the waiting period.

Please contact us before any public disclosure.

## What we consider a security issue

- Authentication or session bypass
- Node isolation bypass (seeing or changing another subtree's data without an invitation)
- Authorization flaws (privilege escalation, missing permission checks)
- Status page access bypass (login, password, magic link, or share token)
- Probe trust issues (a rogue probe reading other teams' monitors, token theft or replay)
- Server-side request forgery through monitors, webhooks, or notification channels
- Injection vulnerabilities (SQL, command, template)
- Path traversal or unsafe file handling, including in the Uptime Kuma import
- Sensitive data exposure (push tokens, notification secrets, user data)
- Release integrity issues (tampered artifacts or images, supply chain)

## What usually does not qualify

- Cosmetic UI defects
- Performance issues without a plausible security impact
- Monitors that can reach internal addresses when an authorized administrator configured them to
- Misconfiguration by an authorized administrator that matches documented behavior
- Dependency version bumps, unless they introduce a new vulnerability surface in v1per
- Unsupported environments

If you are unsure whether something qualifies, report it privately first. We would rather triage a non-issue than miss a real one.

## Other platforms

We do not monitor third-party vulnerability disclosure platforms. All reports must come through GitHub Security Advisories or `security@coderscantina.com`. Direct messages to maintainers on other channels are not a supported reporting path.
