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
