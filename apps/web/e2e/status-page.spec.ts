import { join } from 'node:path'

import { expect, request, test, type APIRequestContext, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'

import type { Commands } from '../src/generated/commands'
import { REQUEST_MARKER_HEADER, REQUEST_MARKER_VALUE } from '../src/generated/limits'
import { startTarget } from './fakes'
import { expectAccessible, expectNoHorizontalOverflow, schemes, settle, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 09, the public status page against a real server. The setup builds two pages
 * through the admin API: a public one with an HTTP monitor against a local target, a manual
 * monitor, a static component and an internal monitor; and a restricted one with a password,
 * an allowed address and a share token. The journeys read the page as visitors do.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const LIVE_WAIT = { timeout: 90_000 }
const PASSWORD = 'open sesame 42'
const run = Date.now().toString(36)
const publicSlug = `status-${run}`
const privateSlug = `partner-${run}`
const ids = { root: '', api: '', website: '', worker: '', support: '', page: '', private: '' }
type CommandName = keyof Commands
type CommandArgs<N extends CommandName> = Commands[N]['args']
type CommandResult<N extends CommandName> = Commands[N]['result']
let shareToken = ''
let api: APIRequestContext
let target: Awaited<ReturnType<typeof startTarget>>

async function command<N extends CommandName>(name: N, args: CommandArgs<N>): Promise<CommandResult<N>> {
  const response = await api.post(`/api/commands/${name}`, { data: { idempotency_key: crypto.randomUUID(), ...args } })
  expect(response.status(), `${name}: ${await response.text()}`).toBe(200)
  return (await response.json()) as CommandResult<N>
}

const shots = process.env.V1PER_E2E_SHOTS

/** A full-page screenshot into `V1PER_E2E_SHOTS`, when set. */
async function capture(page: Page, name: string, fullPage = true) {
  if (!shots) return
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`), fullPage })
}

async function visit(browser: Browser, path: string, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: viewports.desktop, ...options })
  const page = await context.newPage()
  await page.goto(path)
  return page
}

const component = (page: Page, name: string) => page.locator(`[data-component="${name}"]`)
const componentStatus = (page: Page, name: string) => component(page, name).getByTestId('component-status')

test.beforeAll(async ({ baseURL }) => {
  if (!baseURL) throw new Error('no baseURL')
  target = await startTarget()
  api = await request.newContext({
    baseURL,
    storageState: sessionFile,
    extraHTTPHeaders: { [REQUEST_MARKER_HEADER]: REQUEST_MARKER_VALUE, Origin: new URL(baseURL).origin },
  })
  const tree = await api.get('/api/queries/nodes.tree')
  const nodes = ((await tree.json()) as { nodes: { id: string; depth: number }[] }).nodes
  const root = nodes.find((node) => node.depth === 0)?.id
  if (!root) throw new Error('no root node')
  ids.root = root

  const monitor = async (name: string, settings: CommandArgs<'monitors.create'>['settings']) =>
    (
      await command('monitors.create', {
        node: root,
        name,
        description: '',
        settings,
        interval: 20,
        max_retries: 0,
        resend_interval: 0,
        upside_down: false,
        quorum: 'majority',
        tags: [],
        probes: [],
      })
    ).monitor.id
  ids.api = await monitor(`API ${run}`, {
    type: 'http',
    url: target.url,
    method: 'GET',
    headers: [],
    http_body_encoding: 'json',
    auth: { auth_method: 'none' },
    accepted_statuscodes: ['200-299'],
    maxredirects: 10,
    ignore_tls: false,
    cache_bust: false,
  })
  ids.website = await monitor(`Website ${run}`, { type: 'manual' })
  ids.worker = await monitor(`Worker ${run}`, { type: 'manual' })
  await command('monitors.setManualStatus', { monitor: ids.website, status: 'up' })
  await command('monitors.setManualStatus', { monitor: ids.worker, status: 'up' })
  ids.support = (
    await command('components.create', { node: root, name: `Support ${run}`, description: 'Phone and email support' })
  ).component.id

  const created = await command('pages.create', {
    node: root,
    slug: publicSlug,
    title: 'Rockhouse status',
    description: 'Live status of the Rockhouse booking services.',
    footer: 'Questions? Write to ops@rockhouse.example.',
    accent: 'blue',
    theme: 'auto',
    default_locale: 'en',
    locales: ['en', 'de'],
  })
  ids.page = created.page.id
  const laid = await command('pages.updateLayout', {
    page: ids.page,
    expected_revision: created.page.revision,
    groups: [
      {
        name: 'Booking',
        components: [
          { target: { type: 'monitor', monitor: ids.api }, display_name: 'API' },
          { target: { type: 'monitor', monitor: ids.website }, display_name: 'Website' },
          { target: { type: 'component', component: ids.support }, display_name: 'Support' },
        ],
      },
      { name: 'Backend', components: [{ target: { type: 'monitor', monitor: ids.worker }, display_name: 'Worker', internal: true }] },
    ],
  })
  await command('pages.updateAccess', { page: ids.page, expected_revision: laid.page.revision, public: true })

  const restricted = await command('pages.create', { node: root, slug: privateSlug, title: 'Partner status', locales: ['en', 'de'] })
  ids.private = restricted.page.id
  const restrictedLaid = await command('pages.updateLayout', {
    page: ids.private,
    expected_revision: restricted.page.revision,
    groups: [{ name: '', components: [{ target: { type: 'monitor', monitor: ids.api }, display_name: 'Partner API' }] }],
  })
  await command('pages.updateAccess', {
    page: ids.private,
    expected_revision: restrictedLaid.page.revision,
    public: false,
    password: { value: PASSWORD, set: true },
    emails: ['partner@example.test'],
  })
  shareToken = (await command('pages.createShareToken', { page: ids.private, name: 'Partner' })).secret ?? ''

  // The HTTP monitor's first check.
  await expect
    .poll(
      async () => {
        const snapshot = (await (await api.get(`/s/${publicSlug}/snapshot.json`)).json()) as {
          groups: { components: { name: string; status?: string }[] }[]
        }
        return snapshot.groups[0]?.components.find((entry) => entry.name === 'API')?.status
      },
      LIVE_WAIT
    )
    .toBe('operational')
})

test.afterAll(async () => {
  await target?.stop()
  await api?.dispose()
})

test('the page renders from its injected JSON, with keyboard-reachable bars', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop })
  const page = await context.newPage()
  const reads: string[] = []
  const scripts: string[] = []
  page.on('request', (sent) => {
    if (sent.url().includes('/snapshot.json')) reads.push(sent.url())
    if (sent.resourceType() === 'script') scripts.push(new URL(sent.url()).pathname)
  })
  await page.goto(`/s/${publicSlug}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Rockhouse status' })).toBeVisible()
  await expect(page.getByTestId('page-status')).toContainText('All systems operational')
  expect(reads).toEqual([])
  expect(scripts.some((path) => /\/assets\/(admin|vue-app|reka|query|icons)-/.test(path))).toBe(false)
  await expect(page.locator('script#v1per-page')).toHaveCount(1)

  await expect(component(page, 'API').locator('[data-day]')).toHaveCount(90)
  await expect(componentStatus(page, 'Support')).toHaveText('Operational')
  await expect(component(page, 'Worker')).toHaveCount(0)
  await expect(page.getByTestId('internal-detail')).toHaveCount(0)
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()

  // One tab stop per strip, today first; arrows move by day, and the label says it all.
  const strip = page.getByRole('group', { name: 'Last 90 days of API' })
  await expect(strip.locator('[tabindex="0"]')).toHaveCount(1)
  const today = strip.locator('[data-day]').last()
  await today.focus()
  // Today fills in from the day's rollup, so right after the first check it may have no data yet.
  await expect(today).toHaveAttribute('aria-label', /^\w{3} \d+, \d{4}: (No data|Operational, 100% uptime)$/)
  await expect(page.getByTestId('bar-popover')).toBeVisible()
  await page.keyboard.press('ArrowLeft')
  const yesterday = strip.locator('[data-day]').nth(88)
  await expect(yesterday).toBeFocused()
  await expect(yesterday).toHaveAttribute('aria-label', /No data$/)
  await page.keyboard.press('End')
  await expect(today).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('bar-popover')).toHaveCount(0)
  // A static component has a status from the day it was created, today here.
  await component(page, 'Support').locator('[data-day]').last().hover()
  await expect(page.getByTestId('bar-popover')).toContainText('Operational')
  await capture(page, 'bar-tooltip-desktop-light', false)
  await context.close()

  for (const [name, viewport] of Object.entries(viewports)) {
    for (const colorScheme of schemes) {
      const shown = await visit(browser, `/s/${publicSlug}`, { viewport, colorScheme })
      await expect(shown.getByTestId('page-status')).toBeVisible()
      await expect(shown.locator('html')).toHaveClass(colorScheme === 'dark' ? /dark/ : /^(?!.*dark)/)
      await expectNoHorizontalOverflow(shown)
      await expectAccessible(shown)
      await capture(shown, `public-${name}-${colorScheme}`)
      await shown.context().close()
    }
  }
})

test('a visitor whose browser asks for German reads the page in German', async ({ browser }) => {
  const page = await visit(browser, `/s/${publicSlug}`, { locale: 'de-DE' })
  await expect(page.locator('html')).toHaveAttribute('lang', 'de')
  await expect(page.getByTestId('page-status')).toContainText('Alle Systeme betriebsbereit')
  await expect(componentStatus(page, 'API')).toHaveText('Betriebsbereit')
  await expect(page.getByRole('navigation', { name: 'Sprache' }).getByRole('link', { name: 'English' })).toBeVisible()
  await expectAccessible(page)
  await capture(page, 'public-de-desktop-light')
  // The switch asks the server for the other language.
  await page.getByRole('link', { name: 'English' }).click()
  await expect(page.getByTestId('page-status')).toContainText('All systems operational')
  await page.context().close()
})

test('a grant holder sees internal components, response times and probes', async ({ browser }) => {
  for (const [name, viewport] of Object.entries(viewports)) {
    for (const colorScheme of schemes) {
      const page = await visit(browser, `/s/${publicSlug}`, { viewport, colorScheme, storageState: sessionFile })
      await expect(page.getByTestId('internal-note')).toBeVisible()
      await expect(component(page, 'Worker')).toBeVisible()
      const detail = component(page, 'API').getByTestId('internal-detail')
      await expect(detail).toContainText('Response time, last 24 hours')
      // Hourly points come from rollups, so a fresh monitor may have none yet.
      await expect(detail).toContainText(/over the last 24 hours|No response times in the last 24 hours\./)
      await expect(detail.locator('[data-probe]').first()).toContainText('Up')
      await expectAccessible(page)
      await capture(page, `internal-${name}-${colorScheme}`)
      await page.context().close()
    }
  }
})

test('a published incident and maintenance arrive on an open page', async ({ browser }) => {
  const page = await visit(browser, `/s/${publicSlug}`)
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()

  const opened = await command('incidents.create', {
    node: ids.root,
    title: 'Support lines are busy',
    message: 'Calls wait longer than usual.',
    impacts: [{ target: { type: 'component', component: ids.support }, impact: 'partial_outage' }],
  })
  await command('incidents.publish', { incident: opened.incident.id, page: ids.page })
  await command('incidents.addUpdate', {
    incident: opened.incident.id,
    state: 'identified',
    message: 'A phone provider outage. Email works.',
    impacts: [{ target: { type: 'component', component: ids.support }, impact: 'degraded' }],
  })
  const incident = page.locator('[data-incident="Support lines are busy"]')
  await expect(incident).toBeVisible(LIVE_WAIT)
  await expect(incident).toContainText('A phone provider outage. Email works.', LIVE_WAIT)
  await expect(incident.getByTestId('incident-state')).toHaveText('Identified')
  await expect(incident.locator('[data-affected="Support"] [data-impact="degraded"]')).toBeVisible()
  await incident.getByRole('button', { name: 'Show all updates' }).click()
  await expect(incident.locator('[data-update]')).toHaveCount(1)
  await expect(page.getByTestId('page-status')).toContainText('Degraded performance')
  await expect(componentStatus(page, 'Support')).toHaveText('Degraded')

  const node = ids.root
  await command('maintenance.create', {
    node,
    title: 'Website deploy',
    description: 'The website may answer slowly.',
    schedule: { strategy: 'manual' },
    monitors: [ids.website],
  })
  const start = new Date(Date.now() + 3 * 86_400_000)
  const end = new Date(start.getTime() + 2 * 3_600_000)
  const local = (at: Date) => at.toISOString().slice(0, 16)
  await command('maintenance.create', {
    node,
    title: 'Database upgrade',
    schedule: { strategy: 'single', start_date: local(start), end_date: local(end), timezone: 'UTC' },
    monitors: [ids.website],
  })
  const now = page.locator('[data-maintenance="Website deploy"]')
  await expect(now).toContainText('In progress', LIVE_WAIT)
  await expect(page.locator('[data-maintenance="Database upgrade"]')).toContainText('Scheduled', LIVE_WAIT)
  await expect(componentStatus(page, 'Website')).toHaveText('Maintenance', LIVE_WAIT)
  await page.context().close()

  for (const [name, viewport] of Object.entries(viewports)) {
    for (const colorScheme of schemes) {
      const shown = await visit(browser, `/s/${publicSlug}`, { viewport, colorScheme })
      await expect(shown.locator('[data-incident="Support lines are busy"]')).toBeVisible()
      await expectNoHorizontalOverflow(shown)
      await expectAccessible(shown)
      await capture(shown, `incident-maintenance-${name}-${colorScheme}`)
      await shown.context().close()
    }
  }
})

test('the status flips live when the target stops', async ({ browser }) => {
  const page = await visit(browser, `/s/${publicSlug}`)
  await expect(componentStatus(page, 'API')).toHaveText('Operational')
  await page.evaluate(() => Object.assign(window, { sameDocument: true }))
  await target.stop()
  await expect(componentStatus(page, 'API')).toHaveText('Major outage', LIVE_WAIT)
  await expect(page.getByTestId('page-status')).toContainText('Major outage')
  expect(await page.evaluate(() => 'sameDocument' in window)).toBe(true)
  await page.context().close()
})

test('a restricted page opens with its password', async ({ browser }) => {
  for (const [name, viewport] of Object.entries(viewports)) {
    for (const colorScheme of schemes) {
      const gate = await visit(browser, `/s/${privateSlug}`, { viewport, colorScheme })
      await expect(gate.getByRole('heading', { level: 1, name: 'This page is private' })).toBeVisible()
      await expectAccessible(gate)
      await capture(gate, `access-password-${name}-${colorScheme}`)
      await gate.context().close()
    }
  }

  const page = await visit(browser, `/s/${privateSlug}`)
  await expect(page.getByText('Partner status', { exact: true })).toBeVisible()
  await expect(page.getByRole('link', { name: 'Sign in' })).toHaveAttribute('href', `/login?redirect=/s/${privateSlug}`)
  await page.getByLabel('Password').fill('not it')
  await page.getByRole('button', { name: 'Open page' }).click()
  await expect(page.getByRole('alert')).toHaveText('Wrong password.')
  await expect(page.getByLabel('Password')).toHaveAttribute('aria-invalid', 'true')
  await capture(page, 'access-password-error-desktop-light', false)
  await page.getByLabel('Password').fill(PASSWORD)
  await page.getByRole('button', { name: 'Open page' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Partner status' })).toBeVisible()
  await expect(component(page, 'Partner API')).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
  // The page cookie keeps it open.
  await page.reload()
  await expect(page.getByRole('heading', { level: 1, name: 'Partner status' })).toBeVisible()
  await page.context().close()
})

test('a sign-in link is redeemed only after Continue, and a used one says so', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop })
  const page = await context.newPage()
  const redeems: string[] = []
  page.on('request', (sent) => {
    if (sent.url().endsWith('/access/email/redeem')) redeems.push(sent.url())
  })
  await page.goto(`/s/${privateSlug}/access/email?code=not-a-real-code`)
  const next = page.getByRole('button', { name: 'Continue' })
  await expect(next).toBeVisible()
  await expect(page.getByLabel('Email')).toHaveCount(0)
  await settle(page)
  expect(redeems).toEqual([])
  expect(new URL(page.url()).search).toBe('?code=not-a-real-code')
  await expectAccessible(page)

  await next.click()
  await expect(page.getByTestId('code-error')).toHaveText('This link has expired or was already used. Request a new one.')
  expect(redeems).toHaveLength(1)
  expect(new URL(page.url()).search).toBe('')
  await page.getByLabel('Email').fill('partner@example.test')
  await page.getByRole('button', { name: 'Send link' }).click()
  await expect(page.getByTestId('email-sent')).toBeVisible()
  await expectAccessible(page)
  await capture(page, 'access-email-desktop-light')
  await context.close()
})

test('a share token opens a restricted page and keeps it open', async ({ browser }) => {
  expect(shareToken).not.toBe('')
  const page = await visit(browser, `/s/${privateSlug}/t/${shareToken}`, { viewport: viewports.phone })
  await expect(page.getByRole('heading', { level: 1, name: 'Partner status' })).toBeVisible()
  await expect(page.getByTestId('internal-note')).toHaveCount(0)
  await page.goto(`/s/${privateSlug}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Partner status' })).toBeVisible()
  await page.context().close()
})

test('resolved incidents load from the server, ten at a time', async ({ browser }) => {
  for (let index = 1; index <= 12; index++) {
    const opened = await command('incidents.create', {
      node: ids.root,
      title: `Past incident ${String(index).padStart(2, '0')}`,
      impacts: [{ target: { type: 'component', component: ids.support }, impact: 'degraded' }],
    })
    await command('incidents.publish', { incident: opened.incident.id, page: ids.page })
    await command('incidents.addUpdate', { incident: opened.incident.id, state: 'resolved', message: 'Fixed.' })
  }
  const context = await browser.newContext({ viewport: viewports.desktop })
  const page = await context.newPage()
  const reads: string[] = []
  page.on('request', (sent) => {
    if (sent.url().includes('/incidents.json')) reads.push(new URL(sent.url()).search)
  })
  await page.goto(`/s/${publicSlug}`)
  const rows = page.locator('[data-incident^="Past incident"]')
  await expect(rows).toHaveCount(10)
  expect(reads).toHaveLength(1)
  await page.getByRole('button', { name: 'Show more' }).click()
  await expect(rows).toHaveCount(12)
  expect(reads).toHaveLength(2)
  expect(reads[1]).toContain('cursor=')
  await expect(page.getByRole('button', { name: 'Show more' })).toHaveCount(0)
  await expect(rows.first()).toHaveAttribute('data-incident', 'Past incident 12')
  await expectAccessible(page)
  await context.close()
})
