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
  await expect(page.getByRole('navigation', { name: 'Where you are' })).toHaveText('Root')
  await expect(page.getByRole('navigation', { name: 'Mode' }).getByRole('link', { name: 'Observe' })).toHaveAttribute('aria-current', 'page')
  await expect
    .poll(() => streams.opened.at(-1)?.toSorted(), { message: 'the latest stream carries every topic' })
    .toEqual(expect.arrayContaining(['incidents:1', 'maintenance:1', 'monitor-tree:1', expect.stringMatching(/^user:/)]))
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

  // The dashboard follows the tree's monitors and the incidents and maintenance below the node.
  await page.goto(`/n/${node}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Many panels' })).toBeVisible()
  await expect(page.getByText('No open incidents')).toBeVisible()
  await expect.poll(() => streams.open.size, { message: 'one stream open' }).toBe(1)

  await command('incidents.create', { node: child.node?.id, title: 'Below is slow', impacts: [] })
  await expect(page.getByRole('link', { name: /Below is slow/ })).toBeVisible()

  // Pickers on the editors are live too, and the app still answers while they are. The monitor
  // editor follows more topics than the six connections a browser opens per host.
  const modes = page.getByRole('navigation', { name: 'Mode' })
  await page.getByRole('link', { name: 'All incidents' }).click()
  await page.getByRole('link', { name: 'New incident' }).first().click()
  await expect(page).toHaveURL(/\/incidents\/new$/)
  await modes.getByRole('link', { name: 'Configure' }).click()
  await page.getByRole('link', { name: 'Add monitor' }).first().click()
  await expect(page).toHaveURL(/\/monitors\/new$/)
  await expect.poll(() => streams.opened.at(-1)?.length ?? 0).toBeGreaterThan(6)
  await modes.getByRole('link', { name: 'Observe' }).click()
  await expect(page.getByRole('link', { name: /Below is slow/ })).toBeVisible()
  await expect.poll(() => streams.open.size, { message: 'one stream open' }).toBe(1)
  await page.context().close()
})

test('a folder is named in place in the tree and can be dragged right away', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop, storageState: sessionFile })
  const page = await context.newPage()
  const { node } = await ownNode(page, 'Folders')
  await page.goto(`/n/${node}/monitors`)
  const tree = page.getByRole('navigation', { name: 'Nodes and monitors' })
  const add = page.getByRole('button', { name: 'New folder in Folders' }).first()
  const field = tree.getByRole('textbox', { name: 'Folder name' })

  await add.click()
  await expect(field).toBeFocused()
  await field.press('Escape')
  await expect(field).toBeHidden()

  await add.click()
  await field.fill('Inside')
  await field.press('Enter')
  await expect(field).toBeHidden()
  await expect(tree.locator('li', { has: page.getByRole('link', { name: /^Inside/ }) })).toHaveAttribute('draggable', 'true')
  await page.context().close()
})

test('a double click renames a folder and a monitor in place', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop, storageState: sessionFile })
  const page = await context.newPage()
  const { node } = await ownNode(page, 'Renames')
  const created = await page.request.post('/api/commands/monitors.create', {
    headers: { 'x-v1per-request': '1', origin: new URL(page.url()).origin },
    data: {
      idempotency_key: crypto.randomUUID(),
      node,
      name: 'Billing',
      description: '',
      settings: { type: 'manual' },
      interval: 60,
      max_retries: 0,
      resend_interval: 0,
      upside_down: false,
      quorum: 'majority',
      tags: [],
      probes: [],
    },
  })
  expect(created.ok()).toBe(true)
  const { monitor: billing } = (await created.json()) as { monitor: { id: string } }
  await page.goto(`/n/${node}/monitors`)
  const tree = page.getByRole('navigation', { name: 'Nodes and monitors' })

  await tree.getByRole('link', { name: /^Renames/ }).dblclick()
  const folder = tree.getByRole('textbox', { name: 'Rename Renames' })
  await expect(folder).toBeFocused()
  await folder.press('Escape')
  await expect(folder).toBeHidden()
  await tree.getByRole('link', { name: /^Renames/ }).dblclick()
  await folder.fill('Renamed')
  await folder.press('Enter')
  await expect(tree.getByRole('link', { name: /^Renamed/ })).toBeVisible()

  await tree.getByRole('link', { name: /^Billing/ }).dblclick()
  const monitor = tree.getByRole('textbox', { name: 'Rename Billing' })
  await monitor.fill('Invoices')
  await monitor.press('Enter')
  await expect(tree.getByRole('link', { name: /^Invoices/ })).toBeVisible()
  await page.reload()
  await expect(tree.getByRole('link', { name: /^Invoices/ })).toBeVisible()
  await expect(tree.getByRole('link', { name: /^Renamed/ })).toBeVisible()
  const removed = await page.request.post('/api/commands/monitors.delete', {
    headers: { 'x-v1per-request': '1', origin: new URL(page.url()).origin },
    data: { idempotency_key: crypto.randomUUID(), monitor: billing.id },
  })
  expect(removed.ok()).toBe(true)
  await page.context().close()
})

test('the shell at desktop and phone width in light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await signedIn(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      await expect(page.getByRole('heading', { level: 1, name: 'Root' })).toBeVisible()
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
  await page.getByRole('menuitem', { name: 'Appearance' }).press('Enter')
  await expect(page).toHaveURL(/\/account\/appearance$/)
  const modes = page.getByRole('radiogroup', { name: 'Mode' })
  await modes.getByRole('radio', { name: 'Dark' }).press('Enter')
  await expect(page.locator('html')).toHaveClass(/dark/)
  await modes.getByRole('radio', { name: 'Dark' }).press('ArrowRight')
  await expect(modes.getByRole('radio', { name: 'System' })).toHaveAttribute('aria-checked', 'true')

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
  await expect(drawer.getByRole('navigation', { name: 'Nodes and monitors' }).getByRole('link', { name: /^Root/ })).toBeVisible()
  await drawer.getByRole('link', { name: 'Instance', exact: true }).click()
  await expect(drawer).toBeHidden()
  await page.context().close()
})
