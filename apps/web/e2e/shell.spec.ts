import { expect, test, type Browser, type Page, type Request } from '@playwright/test'

import { expectAccessible, expectNoHorizontalOverflow, ownNode, schemes, shot, viewports } from './helpers'
import { admin, instanceName, sessionFile } from './session'

/** The admin shell, signed in with the session from global setup. */

async function signedIn(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  const page = await context.newPage()
  await page.goto('/')
  await expect(page.getByRole('heading', { level: 1, name: 'Root' })).toBeVisible()
  return page
}

/** Records `/api/subscribe` requests: the topics of each, and which are still open. */
function watchStreams(page: Page) {
  const opened: string[][] = []
  const open = new Set<Request>()
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.pathname !== '/api/subscribe') return
    opened.push(url.searchParams.getAll('topic').map((entry) => entry.split('@')[0] ?? entry))
    open.add(request)
  })
  const done = (request: Request) => open.delete(request)
  page.on('requestfinished', done)
  page.on('requestfailed', done)
  return { opened, open }
}

test('home opens the root node and follows all its topics on one stream', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop, storageState: sessionFile })
  const page = await context.newPage()
  const streams = watchStreams(page)
  await page.goto('/')

  await expect(page).toHaveURL(/\/n\/\d+$/)
  await expect(page.getByRole('heading', { level: 1, name: 'Root' })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Node: Root', exact: true })).toBeVisible()
  await expect
    .poll(() => streams.opened.at(-1)?.toSorted(), { message: 'the latest stream carries every topic' })
    .toEqual(expect.arrayContaining(['members:1', 'monitors:1', 'node:1', expect.stringMatching(/^user:/)]))
  await expect.poll(() => streams.open.size, { message: 'one stream open' }).toBe(1)
  await expect(page).toHaveTitle(`Root · ${instanceName}`)
  await page.context().close()
})

test('a screen with many live panels stays live and keeps answering', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop, storageState: sessionFile })
  const page = await context.newPage()
  const { node } = await ownNode(page, 'Many panels')
  const origin = new URL(page.url()).origin
  const command = async (name: string, data: object) => {
    const response = await page.request.post(`/api/commands/${name}`, {
      headers: { 'x-v1per-request': '1', origin },
      data: { idempotency_key: crypto.randomUUID(), ...data },
    })
    expect(response.ok()).toBe(true)
    return response.json() as Promise<{ node?: { id: string } }>
  }
  const child = await command('nodes.create', { parent: node, name: 'Below' })
  const streams = watchStreams(page)

  // The overview follows the node, its members, monitors, incidents, maintenance and pages below,
  // more than the six connections a browser opens per host.
  await page.goto(`/n/${node}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Many panels' })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
  await expect(page.getByText('No open incidents')).toBeVisible()
  await expect.poll(() => streams.opened.at(-1)?.length ?? 0).toBeGreaterThan(6)
  await expect.poll(() => streams.open.size, { message: 'one stream open' }).toBe(1)

  await command('incidents.create', { node: child.node?.id, title: 'Below is slow', impacts: [] })
  await expect(page.getByRole('link', { name: /Below is slow/ })).toBeVisible()

  // Pickers on the editors are live too, and the app still answers while they are.
  const nav = page.getByRole('navigation', { name: 'Main navigation' })
  await nav.getByRole('link', { name: 'Incidents', exact: true }).click()
  await page.getByRole('link', { name: 'Open incident' }).first().click()
  await expect(page).toHaveURL(/\/incidents\/new$/)
  await nav.getByRole('link', { name: 'Monitors', exact: true }).click()
  await page.getByRole('link', { name: 'Add monitor' }).first().click()
  await expect(page).toHaveURL(/\/monitors\/new$/)
  await nav.getByRole('link', { name: 'Overview', exact: true }).click()
  await expect(page.getByRole('link', { name: /Below is slow/ })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
  await expect.poll(() => streams.open.size, { message: 'one stream open' }).toBe(1)
  await page.context().close()
})

test('the shell at desktop and phone width in light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await signedIn(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
      await expectNoHorizontalOverflow(page)
      await expectAccessible(page)
      await shot(page, `root-${size}-${scheme}`)
    }
    await page.context().close()
  }
})

test('keyboard: skip link, user menu, appearance and command palette', async ({ browser }) => {
  const page = await signedIn(browser)

  await page.keyboard.press('Tab')
  await expect(page.getByRole('link', { name: 'Skip to content' })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(page.locator('#main-content')).toBeFocused()

  const menu = page.getByRole('button', { name: /Account menu$/ })
  await menu.focus()
  await page.keyboard.press('Enter')
  await expect(page.getByText(admin.email)).toBeVisible()
  await shot(page, 'user-menu-desktop-light')
  await page.getByRole('menuitem', { name: 'Appearance' }).press('ArrowRight')
  await page.getByRole('menuitemradio', { name: 'Dark' }).press('Enter')
  await expect(page.locator('html')).toHaveClass(/dark/)
  await expect(menu).toBeFocused()

  await menu.press('Enter')
  await page.getByRole('menuitem', { name: 'Appearance' }).press('ArrowRight')
  await page.getByRole('menuitemradio', { name: 'System' }).press('Enter')
  await page.keyboard.press('Escape')

  // The test browser reports a Windows user agent, so the shortcut is Ctrl+K.
  await page.keyboard.press('Control+k')
  const search = page.getByPlaceholder('Search pages and actions')
  await expect(search).toBeFocused()
  await page.keyboard.type('Members')
  await expect(page.getByRole('option', { name: 'Members' })).toHaveAttribute('data-highlighted', '')
  await shot(page, 'command-palette-desktop-light')
  await page.keyboard.press('Escape')
  await expect(search).toBeHidden()
  await page.context().close()
})

test('the phone drawer opens from the tab bar and closes on navigation', async ({ browser }) => {
  const page = await signedIn(browser, viewports.phone)
  await page.getByRole('navigation', { name: 'Phone navigation' }).getByRole('button', { name: 'Menu' }).click()
  const drawer = page.getByRole('dialog', { name: 'Main navigation' })
  await expect(drawer).toBeVisible()
  await shot(page, 'drawer-phone-light')
  await drawer.getByRole('link', { name: 'Members' }).click()
  await expect(drawer).toBeHidden()
  await page.context().close()
})
