import type { PageBoot } from '~/generated'

/**
 * The `PageBoot` the server put into `<script id="v1per-page">`, or null when the HTML came
 * without one (the file opened directly, or a broken proxy).
 */
export function readBoot(): PageBoot | null {
  const text = document.getElementById('v1per-page')?.textContent
  if (!text) return null
  try {
    const boot = JSON.parse(text) as PageBoot
    return boot.kind === 'page' || boot.kind === 'gate' ? boot : null
  } catch {
    return null
  }
}

/** The `code` of a sign-in link, when the page was opened at `<base>/access/email?code=`. */
export function linkCode(base: string): string | null {
  if (window.location.pathname !== `${base}/access/email`) return null
  return new URLSearchParams(window.location.search).get('code')
}

/** A subscriber's mailed link: `<base>/subscription?token=`, with `action=unsubscribe` to leave. */
export function subscriptionLink(base: string): { token: string; unsubscribe: boolean } | null {
  if (window.location.pathname !== `${base}/subscription`) return null
  const query = new URLSearchParams(window.location.search)
  const token = query.get('token')
  return token ? { token, unsubscribe: query.get('action') === 'unsubscribe' } : null
}
