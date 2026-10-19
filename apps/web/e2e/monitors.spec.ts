import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test, type Browser, type Page } from '@playwright/test'

import { configure, expectAccessible, expectNoHorizontalOverflow, schemes, shot, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 05 journeys against a real server and its built-in probe: an admin adds an http
 * monitor for a local target the test runs, sees it go up, stops the target and sees it go
 * down; then adds a push monitor and pushes to it like a cron job would. The checks run on the
 * shortest interval (20 s), so status changes take up to one interval to show.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const STATUS_WAIT = { timeout: 60_000 }
const ids: Record<string, string> = {}
let target: Server | null = null
let targetUrl = ''

/** A plain HTTP endpoint that answers 200, standing in for a website. */
async function startTarget(): Promise<Server> {
  const server = createServer((_, response) => {
    response.writeHead(200, { 'content-type': 'text/plain' })
    response.end('ok')
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  return server
}

async function stopTarget() {
  const server = target
  target = null
  if (!server) return
  server.closeAllConnections()
  await new Promise((done) => server.close(done))
}

test.afterAll(stopTarget)

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

const statusPanel = (page: Page) => page.getByRole('region', { name: 'Current status' })

async function chooseType(page: Page, type: string) {
  await page.getByRole('combobox', { name: 'Type' }).click()
  await page.getByRole('option', { name: type, exact: true }).click()
}

test('an http monitor goes up, and down when its target stops', async ({ browser }) => {
  target = await startTarget()
  targetUrl = `http://127.0.0.1:${(target.address() as AddressInfo).port}/health`

  const page = await asAdmin(browser)
  await page.goto('/')
  await expect(page).toHaveURL(/\/n\/\d+$/)
  ids.root = new URL(page.url()).pathname.split('/')[2] ?? ''

  await configure(page, 'Monitors')
  await expect(page.getByRole('heading', { level: 1, name: 'Monitors' })).toBeVisible()
  await expect(page.getByText('No monitors on this node yet')).toBeVisible()
  await shot(page, 'list-empty-desktop-light')

  await page.getByRole('link', { name: 'Add monitor' }).first().click()
  await expect(page.getByRole('heading', { level: 1, name: 'Add monitor' })).toBeVisible()
  await expect(page.getByRole('combobox', { name: 'Type' })).toHaveText(/HTTP/)

  // The browser refuses an empty name before anything is sent.
  await page.getByRole('button', { name: 'Add monitor' }).click()
  await expect(page.getByLabel('Name', { exact: true })).toHaveAttribute('aria-invalid', 'true')

  await page.getByLabel('Name', { exact: true }).fill('Web shop')
  await page.getByLabel('Tags').fill('shop, web')
  await page.getByLabel('URL', { exact: true }).fill(targetUrl)
  await page.getByLabel('Interval in seconds', { exact: true }).fill('20')
  await page.getByRole('button', { name: 'Add monitor' }).click()

  await expect(page).toHaveURL(/\/monitors\/\d+$/)
  ids.http = new URL(page.url()).pathname.split('/')[4] ?? ''
  await expect(page.getByRole('heading', { level: 1, name: 'Web shop' })).toBeVisible()
  await expect(statusPanel(page).locator('[data-status="up"]')).toBeVisible(STATUS_WAIT)
  await expect(page.getByRole('table', { name: 'Checks' }).locator('[data-status="up"]').first()).toBeVisible()

  await stopTarget()
  await expect(statusPanel(page).locator('[data-status="down"]')).toBeVisible(STATUS_WAIT)
  await expect(page.getByRole('table', { name: 'Checks' }).locator('[data-status="down"]').first()).toBeVisible(STATUS_WAIT)
  await page.context().close()
})

test('a push monitor records the pushes a job sends', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.root}/monitors/new`)
  await chooseType(page, 'Push')
  await expect(page.getByText('The push URL shows here after you add the monitor.')).toBeVisible()
  await page.getByLabel('Name', { exact: true }).fill('Nightly backup')
  await page.getByRole('button', { name: 'Add monitor' }).click()

  await expect(page.getByRole('heading', { level: 1, name: 'Nightly backup' })).toBeVisible()
  ids.push = new URL(page.url()).pathname.split('/')[4] ?? ''
  const url = await page.getByRole('textbox', { name: 'Push URL' }).inputValue()
  expect(url).toMatch(/\/api\/push\/[A-Za-z0-9]{32}\?status=up&msg=OK&ping=$/)
  await expect(statusPanel(page).locator('[data-status="unknown"]')).toBeVisible()

  const pushed = await fetch(`${url}42`)
  expect(pushed.status).toBe(200)
  expect(await pushed.json()).toEqual({ ok: true })
  await expect(statusPanel(page).locator('[data-status="up"]')).toBeVisible(STATUS_WAIT)
  await expect(statusPanel(page).getByTestId('monitor-message')).toContainText('OK')

  const failed = await fetch(url.replace('status=up&msg=OK', 'status=down&msg=disk%20full'))
  expect(failed.status).toBe(200)
  await expect(statusPanel(page).locator('[data-status="down"]')).toBeVisible(STATUS_WAIT)
  await expect(statusPanel(page).getByTestId('monitor-message')).toContainText('disk full')
  await page.context().close()
})

test('the dashboard lists what needs attention, and the list filters by status', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.root}`)
  const attention = page.getByRole('region', { name: 'Needs attention' })
  await expect(attention.getByRole('link', { name: 'Web shop' })).toBeVisible()
  await expect(attention.getByRole('link', { name: 'Nightly backup' })).toBeVisible()

  await page.goto(`/n/${ids.root}/monitors?status=down`)
  const table = page.getByRole('table', { name: 'Monitors' })
  await expect(table.getByRole('link', { name: 'Web shop' })).toBeVisible()
  await expect(table.getByRole('link', { name: 'Nightly backup' })).toBeVisible()

  await page.getByRole('combobox', { name: 'Type' }).click()
  await page.getByRole('option', { name: 'Push', exact: true }).click()
  await expect(table.getByRole('link', { name: 'Web shop' })).toBeHidden()
  await page.getByPlaceholder('Search monitors').fill('nothing like this')
  await expect(table.getByText('No monitor matches these filters.')).toBeVisible()
  await page.getByRole('button', { name: 'Clear filters' }).click()
  await expect(table.getByRole('link', { name: 'Web shop' })).toBeVisible()
  await page.context().close()
})

test('a sealed credential shows only that it is set, and can be replaced or cleared', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.root}/monitors/new`)
  await page.getByLabel('Name', { exact: true }).fill('Status API')
  await page.getByLabel('URL', { exact: true }).fill('http://127.0.0.1:9/health')
  await page.getByRole('combobox', { name: 'Authentication' }).click()
  await page.getByRole('option', { name: 'Bearer token' }).click()
  await page.getByLabel('Token', { exact: true }).fill('very-secret-token')
  await page.getByRole('button', { name: 'Add monitor' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Status API' })).toBeVisible()
  ids.secret = new URL(page.url()).pathname.split('/')[4] ?? ''

  await page.getByRole('link', { name: 'Edit' }).click()
  await expect(page.getByText('Set', { exact: true })).toBeVisible()
  await expect(page.getByText('very-secret-token')).toHaveCount(0)
  await page.getByRole('button', { name: 'Clear: Token' }).click()
  await expect(page.getByText('Cleared when you save.')).toBeVisible()
  await page.getByRole('button', { name: 'Keep' }).click()
  await page.getByRole('button', { name: 'Replace: Token' }).click()
  await expect(page.getByLabel('Token', { exact: true })).toHaveValue('')
  await page.getByRole('button', { name: 'Keep' }).click()
  await expect(page.getByText('Set', { exact: true })).toBeVisible()
  await page.context().close()
})

test('monitor screens hold up at desktop and phone sizes, light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.root}/monitors`)
      await expect(page.getByRole('table', { name: 'Monitors' }).getByRole('link', { name: 'Web shop' })).toBeVisible()
      await check('list')

      await page.goto(`/n/${ids.root}/monitors/${ids.http}`)
      await expect(statusPanel(page).locator('[data-status="down"]')).toBeVisible()
      await expect(page.getByRole('table', { name: 'Checks' }).locator('[data-status]').first()).toBeVisible()
      await check('detail-http')

      await page.goto(`/n/${ids.root}/monitors/${ids.push}`)
      await expect(page.getByRole('textbox', { name: 'Push URL' })).toBeVisible()
      await check('detail-push')

      await page.goto(`/n/${ids.root}/monitors/${ids.secret}/edit`)
      await page.getByText('Set', { exact: true }).scrollIntoViewIfNeeded()
      await check('editor-http')

      await page.goto(`/n/${ids.root}/monitors/${ids.push}/edit`)
      await page.getByRole('textbox', { name: 'Push URL' }).scrollIntoViewIfNeeded()
      await check('editor-push')
    }
    await page.context().close()
  }
})
