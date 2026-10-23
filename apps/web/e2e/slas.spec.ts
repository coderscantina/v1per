import { expect, request, test, type APIRequestContext, type Browser, type Page } from '@playwright/test'

import type { Commands } from '../src/generated/commands'
import { REQUEST_MARKER_HEADER, REQUEST_MARKER_VALUE } from '../src/generated/limits'
import type { PageBoot } from '../src/generated/PageBoot'
import type { PageSnapshot } from '../src/generated/PageSnapshot'
import { expectAccessible, expectNoHorizontalOverflow, ownNode, schemes, shot, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 14 journeys against a real server: an admin adds an SLA with business hours to a
 * client node, opens its running period and shows it on a public status page, whose public
 * snapshot carries the SLA aggregates and no incident title. A client Admin on the SLA's own
 * node reads it and cannot change it. The setup goes through the API; the journeys use the app.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const run = Date.now().toString(36)
const slug = `sla-${run}`
const SLA = 'Managed hosting'
const INCIDENT = 'Database failover at night'
const client = { name: 'Carla Client', email: `carla-${run}@example.test`, password: 'carla-has-a-long-password' }
const ids = { node: '', monitor: '', page: '', sla: '' }
let api: APIRequestContext

type CommandName = keyof Commands

async function command<N extends CommandName>(name: N, args: Commands[N]['args']): Promise<Commands[N]['result']> {
  const response = await api.post(`/api/commands/${name}`, { data: { idempotency_key: crypto.randomUUID(), ...args } })
  expect(response.status(), `${name}: ${await response.text()}`).toBe(200)
  return (await response.json()) as Commands[N]['result']
}

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

/** The snapshot the server put into the page's HTML for this visitor. */
async function snapshot(page: Page): Promise<PageSnapshot> {
  await page.goto(`/s/${slug}`)
  const boot = JSON.parse((await page.locator('script#v1per-page').textContent()) ?? 'null') as PageBoot
  if (boot.kind !== 'page') throw new Error(`/s/${slug} answered with a ${boot.kind}`)
  return boot.snapshot
}

test.beforeAll(async ({ baseURL, browser }) => {
  if (!baseURL) throw new Error('no baseURL')
  api = await request.newContext({
    baseURL,
    storageState: sessionFile,
    extraHTTPHeaders: { [REQUEST_MARKER_HEADER]: REQUEST_MARKER_VALUE, Origin: new URL(baseURL).origin },
  })
  const page = await asAdmin(browser)
  ids.node = (await ownNode(page, 'Hosting client')).node
  await page.context().close()

  ids.monitor = (
    await command('monitors.create', {
      node: ids.node,
      name: 'Shop',
      description: '',
      settings: { type: 'manual' },
      interval: 20,
      max_retries: 0,
      resend_interval: 0,
      upside_down: false,
      quorum: 'majority',
      tags: [],
      probes: [],
    })
  ).monitor.id
  await command('monitors.setManualStatus', { monitor: ids.monitor, status: 'up' })
  await command('incidents.create', {
    node: ids.node,
    title: INCIDENT,
    impacts: [{ target: { type: 'monitor', monitor: ids.monitor }, impact: 'major_outage' }],
  })

  const created = await command('pages.create', { node: ids.node, slug, title: 'Hosting status', locales: ['en'] })
  ids.page = created.page.id
  const laid = await command('pages.updateLayout', {
    page: ids.page,
    expected_revision: created.page.revision,
    groups: [{ name: 'Services', components: [{ target: { type: 'monitor', monitor: ids.monitor }, display_name: 'Shop' }] }],
  })
  await command('pages.updateAccess', { page: ids.page, expected_revision: laid.page.revision, public: true })
})

test.afterAll(async () => {
  await api.dispose()
})

test('an admin adds an SLA with business hours and follows its running period', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/slas`)
  await expect(page.getByText('No SLAs', { exact: true })).toBeVisible()
  await page.getByRole('link', { name: 'Add SLA' }).first().click()

  await page.getByLabel('Name', { exact: true }).fill(SLA)
  await page.getByLabel('Contract start').fill(`${new Date().toISOString().slice(0, 7)}-01`)
  await page.getByRole('checkbox', { name: 'Shop' }).check()
  await page.getByLabel('Availability', { exact: true }).fill('99.9')
  await page.getByLabel('Major outage response').fill('30')
  await page.getByLabel('Major outage resolution').fill('240')
  await page.getByRole('button', { name: 'Monday to Friday, 09:00 to 17:00' }).click()
  await expect(page.locator('[data-weekday="1"]').getByLabel('From on Monday')).toHaveValue('09:00')
  await page.getByLabel('Holiday', { exact: true }).fill(`${new Date().getUTCFullYear()}-12-25`)
  await page.getByRole('button', { name: 'Add', exact: true }).click()
  await shot(page, 'sla-editor-desktop-light')
  await page.getByRole('button', { name: 'Add SLA' }).click()

  const row = page.locator(`[data-sla-row="${SLA}"]`)
  await expect(row).toContainText('99.9 % availability')
  await expect(row).toContainText('Major outage: response 30 min, resolution 4 h')
  await row.getByRole('link', { name: SLA, exact: true }).click()
  await expect(page.getByRole('heading', { level: 1, name: SLA })).toBeVisible()
  ids.sla = new URL(page.url()).pathname.split('/')[4] ?? ''

  // The running period is live, holds both commitments and counts the incident on the monitor.
  await expect(page.locator('[data-period-state="live"]')).toBeVisible()
  await expect(page.locator('[data-commitment="availability"]')).toBeVisible()
  await expect(page.locator('[data-commitment="response"]')).toBeVisible()
  await expect(page.locator(`[data-sla-incident="${INCIDENT}"]`)).toBeVisible()
  await shot(page, 'sla-period-desktop-light')
  await page.context().close()
})

test('a status page shows the SLA without incident titles, team members also per monitor', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/pages/${ids.page}/slas`)
  await page.getByRole('button', { name: `Show ${SLA}` }).click()
  await expect(page.locator(`[data-page-sla="${SLA}"]`)).toBeVisible()
  await page.getByRole('button', { name: 'Save SLAs' }).click()
  await expect(page.getByText('Saved.')).toBeVisible()

  const internal = await snapshot(page)
  expect(internal.audience).toBe('internal')
  expect(internal.slas.map((sla) => sla.name)).toEqual([SLA])
  expect(internal.slas[0]?.current?.monitors?.length).toBe(1)
  await page.context().close()

  const visitor = await browser.newContext({ viewport: viewports.desktop })
  const anonymous = await visitor.newPage()
  const open = await snapshot(anonymous)
  expect(open.audience).toBe('public')
  const [sla] = open.slas
  expect(sla?.name).toBe(SLA)
  expect(sla?.current?.availability).toBeDefined()
  expect(sla?.current?.response).toBeDefined()
  expect(sla?.current?.monitors).toBeUndefined()
  expect(JSON.stringify(open.slas)).not.toContain(INCIDENT)
  await expect(anonymous.getByRole('region', { name: 'SLAs' }).locator(`[data-sla="${SLA}"]`)).toBeVisible()
  await expect(anonymous.getByRole('region', { name: 'SLAs' })).not.toContainText(INCIDENT)
  await shot(anonymous, 'sla-status-page-desktop-light')
  await visitor.close()
})

test('a client Admin on the SLA node reads the SLA and cannot change it', async ({ browser, baseURL }) => {
  const invited = await command('members.invite', { node: ids.node, role: 'admin', email: client.email })
  const guest = await (await browser.newContext({ viewport: viewports.desktop })).newPage()
  await guest.goto(invited.secret?.link ?? '')
  await guest.getByLabel('Your name').fill(client.name)
  await guest.getByLabel('Password', { exact: true }).fill(client.password)
  await guest.getByLabel('Repeat password').fill(client.password)
  await guest.getByRole('button', { name: 'Create account and accept' }).click()
  await expect(guest.getByRole('heading', { level: 1, name: 'Hosting client' })).toBeVisible()

  await guest.goto(`/n/${ids.node}/slas`)
  const row = guest.locator(`[data-sla-row="${SLA}"]`)
  await expect(row).toContainText('99.9 % availability')
  await expect(guest.getByRole('link', { name: 'Add SLA' })).toHaveCount(0)
  await expect(row.getByRole('link', { name: `Edit ${SLA}` })).toHaveCount(0)
  await expect(row.getByRole('button', { name: `Delete ${SLA}` })).toHaveCount(0)
  await row.getByRole('link', { name: SLA, exact: true }).click()
  await expect(guest.locator('[data-commitment="availability"]')).toBeVisible()
  await expect(guest.getByRole('link', { name: 'Edit SLA' })).toHaveCount(0)

  await guest.goto(`/n/${ids.node}/slas/${ids.sla}/edit`)
  await expect(guest.getByText('SLAs are set by staff above this node. You can read them.')).toBeVisible()

  // The server refuses the write itself, not only the app.
  const refused = await guest.request.post('/api/commands/slas.delete', {
    headers: { [REQUEST_MARKER_HEADER]: REQUEST_MARKER_VALUE, origin: new URL(baseURL ?? '').origin },
    data: { idempotency_key: crypto.randomUUID(), sla: ids.sla },
  })
  expect(refused.status()).toBe(403)
  await guest.context().close()
})

test('SLA screens hold up at desktop and phone sizes, light and dark', async ({ browser }) => {
  test.skip(ids.sla === '', 'needs the SLA from the first journey')
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme, media: 'screen' })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `sla-${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.node}/slas`)
      await expect(page.locator(`[data-sla-row="${SLA}"]`)).toBeVisible()
      await check('list')

      await page.goto(`/n/${ids.node}/slas/${ids.sla}/edit`)
      await expect(page.getByLabel('Name', { exact: true })).toHaveValue(SLA)
      await check('editor')

      await page.goto(`/n/${ids.node}/slas/${ids.sla}`)
      await expect(page.locator(`[data-sla-incident="${INCIDENT}"]`)).toBeVisible()
      await check('period')

      await page.getByRole('link', { name: 'Report' }).click()
      await expect(page.getByRole('heading', { level: 1 })).toContainText(SLA)
      await page.emulateMedia({ colorScheme: scheme, media: 'print' })
      await check('report-print')
      await page.emulateMedia({ colorScheme: scheme, media: 'screen' })

      await page.goto(`/s/${slug}`)
      await expect(page.locator(`[data-sla="${SLA}"]`)).toBeVisible()
      await expectAccessible(page)
      await shot(page, `sla-status-page-${size}-${scheme}`)
    }
    await page.context().close()
  }
})
