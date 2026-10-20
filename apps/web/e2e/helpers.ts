import { createHmac } from 'node:crypto'
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

import AxeBuilder from '@axe-core/playwright'
import { expect, type Page } from '@playwright/test'

const shots = process.env.V1PER_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

export const schemes = ['light', 'dark'] as const
export const viewports = {
  desktop: { width: 1440, height: 900 },
  phone: { width: 390, height: 844 },
} as const

/**
 * Waits for open and close animations and colour transitions, so a screenshot or a contrast check
 * sees the settled state. Two frames first: a scheme change starts its transitions on the next one.
 * Scroll-driven animations (the table edge fades) follow scrolling, not time, and never finish.
 */
export async function settle(page: Page) {
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => animation.timeline instanceof DocumentTimeline)
        .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
        .map((animation) => animation.finished.catch(() => undefined))
    )
  )
}

/** Writes `<name>.png` into `V1PER_E2E_SHOTS`, when set. */
export async function shot(page: Page, name: string) {
  if (!shots) return
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`) })
}

/**
 * Neither the page nor any visible element inside the main region reaches past the viewport.
 * Content of a horizontal scroller (a wide table on a phone) scrolls in its box and counts once,
 * as the scroller.
 */
export async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const page = document.documentElement.scrollWidth - window.innerWidth
    const scrolled = (element: Element) => {
      for (let at = element.parentElement; at; at = at.parentElement) {
        if (['auto', 'scroll'].includes(getComputedStyle(at).overflowX)) return true
      }
      return false
    }
    const clipped = Array.from(document.querySelectorAll('#main-content *')).filter((element) => {
      const box = element.getBoundingClientRect()
      return box.width > 0 && box.right > window.innerWidth + 1 && !element.closest('.sr-only') && !scrolled(element)
    })
    return { page, clipped: clipped.slice(0, 3).map((element) => element.outerHTML.slice(0, 120)) }
  })
  expect(overflow).toEqual({ page: 0, clipped: [] })
}

/** axe with the WCAG 2.1 A and AA rules finds nothing. */
export async function expectAccessible(page: Page) {
  await settle(page)
  const { violations } = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze()
  const found = violations.flatMap((violation) =>
    violation.nodes.map((node) => `${violation.id}: ${node.html} (${node.failureSummary?.split('\n')[1]?.trim() ?? ''})`)
  )
  expect(found).toEqual([])
}

/**
 * Adds a node below the root the admin opens on and returns both IDs, so a journey works on a
 * node of its own and leaves the root as other journeys expect it. Goes through the API with the
 * page's session, as the browser would.
 */
export async function ownNode(page: Page, name: string): Promise<{ root: string; node: string }> {
  await page.goto('/')
  await expect(page).toHaveURL(/\/n\/\d+$/)
  const url = new URL(page.url())
  const root = url.pathname.split('/')[2] ?? ''
  const response = await page.request.post('/api/commands/nodes.create', {
    headers: { 'x-v1per-request': '1', origin: url.origin },
    data: { idempotency_key: crypto.randomUUID(), parent: root, name },
  })
  expect(response.ok()).toBe(true)
  const { node } = (await response.json()) as { node: { id: string } }
  return { root, node: node.id }
}

/** Opens a section of the current node's configuration through the mode switch and its tabs. */
export async function configure(page: Page, section: string) {
  await page.getByRole('navigation', { name: 'Mode' }).getByRole('link', { name: 'Configure' }).click()
  // From a monitor's page the switch opens its editor, whose bar leads back to the folder.
  const monitor = page.getByRole('navigation', { name: 'Monitor configuration' })
  const node = page.getByRole('navigation', { name: 'Node configuration' })
  await expect(monitor.or(node)).toBeVisible()
  if (await monitor.isVisible()) await monitor.getByRole('link').first().click()
  await page.getByRole('navigation', { name: 'Node configuration' }).getByRole('link', { name: section, exact: true }).click()
}

/** The current RFC 6238 code for a base32 key (SHA1, 6 digits, 30 s), as an authenticator app shows it. */
export function totp(key: string, at = Date.now()): { code: string; step: number } {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
  let bits = ''
  for (const char of key.replace(/=+$/, '').toUpperCase()) bits += alphabet.indexOf(char).toString(2).padStart(5, '0')
  const secret = Buffer.from(bits.match(/.{8}/g)?.map((byte) => Number.parseInt(byte, 2)) ?? [])
  const step = Math.floor(at / 30_000)
  const counter = Buffer.alloc(8)
  counter.writeBigUInt64BE(BigInt(step))
  const digest = createHmac('sha1', secret).update(counter).digest()
  const offset = (digest[digest.length - 1] ?? 0) & 0xf
  const value = (digest.readUInt32BE(offset) & 0x7fffffff) % 1_000_000
  return { code: String(value).padStart(6, '0'), step }
}
