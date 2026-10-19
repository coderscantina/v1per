import { expect, test, type Browser, type Page } from '@playwright/test'

import type { PageBoot } from '../src/generated/PageBoot'
import { expectAccessible, expectNoHorizontalOverflow, ownNode, schemes, shot, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 09 admin journeys against a real server: an admin creates a status page with a
 * monitor and a static component, makes it public and finds both in the page the server
 * serves at `/s/<slug>`; then restricts it with a password, which turns the page into a gate,
 * shares it with a link, and publishes incidents to it, an auto-incident from triage included.
 * Assertions on `/s/` read the JSON the server injects, not the status app's design.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const STATUS_WAIT = { timeout: 60_000 }
const ids: Record<string, string> = {}
const slug = `acme-${Date.now().toString(36)}`

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

/** The boot JSON the server put into the page's HTML. */
async function boot(page: Page): Promise<PageBoot> {
  const text = await page.locator('script#v1per-page').textContent()
  return JSON.parse(text ?? 'null') as PageBoot
}

const componentNames = (data: PageBoot) =>
  data.kind === 'page' ? data.snapshot.groups.flatMap((group) => group.components.map((component) => component.name)) : []

const editor = (tab = 'settings') => `/n/${ids.node}/pages/${ids.page}/${tab}`

test('a page is created with a monitor and a static component', async ({ browser }) => {
  const page = await asAdmin(browser)
  ids.node = (await ownNode(page, 'Status desk')).node

  await page.goto(`/n/${ids.node}/components`)
  await page.getByRole('button', { name: 'Add component' }).first().click()
  await page.getByRole('dialog').getByLabel('Name').fill('Support')
  await page.getByRole('dialog').getByRole('button', { name: 'Add component' }).click()
  await expect(page.locator('[data-component="Support"]')).toBeVisible()

  await page.goto(`/n/${ids.node}/monitors/new`)
  await page.getByRole('combobox', { name: 'Type', exact: true }).click()
  await page.getByRole('option', { name: 'Manual', exact: true }).click()
  await page.getByLabel('Name', { exact: true }).fill('Checkout')
  await page.getByLabel('Interval in seconds', { exact: true }).fill('20')
  await page.getByRole('button', { name: 'Add monitor' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Checkout' })).toBeVisible()
  ids.monitor = new URL(page.url()).pathname.split('/')[4] ?? ''

  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'Status pages' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Status pages' })).toBeVisible()
  await page.getByRole('button', { name: 'Create page' }).first().click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Title').fill('Acme status')
  await expect(dialog.getByLabel('Address')).toHaveValue('acme-status')
  await dialog.getByLabel('Address').fill(slug)
  await dialog.getByRole('button', { name: 'Create page' }).click()

  await expect(page).toHaveURL(/\/pages\/\d+\/layout$/)
  ids.page = new URL(page.url()).pathname.split('/')[4] ?? ''
  await page.getByRole('button', { name: 'Add components' }).click()
  const picker = page.getByRole('dialog')
  await picker.getByRole('checkbox', { name: 'Checkout' }).click()
  await picker.getByRole('checkbox', { name: 'Support' }).click()
  await picker.getByRole('button', { name: 'Add 2' }).click()
  await page.getByRole('textbox', { name: 'Group name' }).fill('Services')
  await page.getByRole('textbox', { name: 'Shown name for Checkout' }).fill('Shop checkout')

  // The arrow buttons reorder from the keyboard and keep focus on the moved row.
  await page.getByRole('button', { name: 'Move Support up' }).click()
  await expect(page.getByRole('button', { name: 'Move Support down' })).toBeFocused()
  await expect(page.locator('[data-component]')).toHaveText([/Support/, /Checkout/])
  await page.getByRole('button', { name: 'Save components' }).click()
  await expect(page.getByText('2 components on the page')).toBeVisible()

  await page.goto(`/n/${ids.node}/pages`)
  const row = page.locator(`[data-page="${slug}"]`)
  await expect(row).toContainText('Acme status')
  await expect(row.locator('[data-access="restricted"]')).toBeVisible()
  await page.context().close()
})

test('a public page serves its components at /s/<slug>, a password turns it into a gate', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(editor('access'))
  await expect(page.getByTestId('access-modes').locator('[data-mode="team"]')).toBeVisible()
  await page.getByRole('switch', { name: 'Anyone with the address can open the page' }).click()
  await page.getByRole('button', { name: 'Save access' }).click()
  await expect(page.getByTestId('access-modes').locator('[data-mode="public"]')).toBeVisible()

  const visitor = await browser.newContext()
  const anonymous = await visitor.newPage()
  await anonymous.goto(`/s/${slug}`)
  const open = await boot(anonymous)
  expect(open.kind).toBe('page')
  expect(componentNames(open)).toEqual(['Support', 'Shop checkout'])

  await page.getByRole('switch', { name: 'Anyone with the address can open the page' }).click()
  await page.getByRole('button', { name: 'Set password' }).click()
  await page.locator('#page-password').fill('open sesame')
  await page.getByRole('button', { name: 'Save access' }).click()
  await expect(page.getByTestId('password-state')).toHaveText('A password is set.')
  await expect(page.getByTestId('access-modes').locator('[data-mode="password"]')).toBeVisible()

  await visitor.close()

  // A fresh visitor: the public page was cacheable for 15 seconds.
  const later = await browser.newContext()
  const stranger = await later.newPage()
  await stranger.goto(`/s/${slug}`)
  expect(await boot(stranger)).toMatchObject({ kind: 'gate', gate: { password: true } })
  await later.close()
  await page.context().close()
})

test('a share link opens the restricted page without signing in', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(editor('access'))
  const links = page.getByRole('region', { name: 'Share links' })
  await links.getByLabel('Name').fill('Client A')
  await links.getByRole('button', { name: 'Create share link' }).click()
  const link = await links.getByLabel('Share link', { exact: true }).inputValue()
  expect(link).toMatch(new RegExp(`/s/${slug}/t/.+`))
  await links.getByRole('button', { name: 'Done' }).click()
  await expect(links.locator('[data-token="Client A"]')).toBeVisible()
  await expect(page.getByTestId('access-modes').locator('[data-mode="tokens"]')).toBeVisible()

  const visitor = await browser.newContext()
  const anonymous = await visitor.newPage()
  await anonymous.goto(link)
  expect(componentNames(await boot(anonymous))).toEqual(['Support', 'Shop checkout'])
  ids.shareLink = link
  await visitor.close()

  // The logo goes up as raw bytes and shows in the settings.
  await page.goto(editor())
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#4f46e5"/></svg>'
  await page.locator('input[type="file"]').setInputFiles({ name: 'logo.svg', mimeType: 'image/svg+xml', buffer: Buffer.from(svg) })
  await expect(page.getByRole('img', { name: 'Page logo' })).toBeVisible()
  await page.getByLabel('Footer').fill('Questions: support@acme.test')
  await page.getByRole('button', { name: 'Save settings' }).click()
  await expect(page.getByText('Saved.')).toBeVisible()

  await page.goto(editor('hostnames'))
  await page.getByLabel('Domain', { exact: true }).fill('status.acme.test')
  await page.getByRole('button', { name: 'Add domain' }).click()
  await expect(page.locator('[data-hostname="status.acme.test"]')).toBeVisible()
  await page.context().close()
})

test('incidents are published to the page, from the detail and from auto-incident triage', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/incidents/new`)
  await page.getByLabel('Title').fill('Payments are slow')
  await page.getByLabel('Message').fill('Card payments take up to a minute.')
  await page.getByRole('group', { name: 'Affected' }).locator('[data-target="Support"]').getByRole('checkbox').click()
  await page.getByRole('button', { name: 'Open incident' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Payments are slow' })).toBeVisible()

  // A manual incident goes to the pages that show what it affects; it can come off and back.
  const pages = page.getByRole('region', { name: 'Status pages' })
  const entry = pages.locator(`[data-publication="${slug}"]`)
  await expect(entry).toHaveAttribute('data-published', 'true')
  await entry.getByRole('button', { name: 'Unpublish from Acme status' }).click()
  await expect(entry).toHaveAttribute('data-published', 'false')
  await entry.getByRole('button', { name: 'Publish to Acme status' }).click()
  await expect(entry).toHaveAttribute('data-published', 'true')

  const visitor = await browser.newContext()
  const anonymous = await visitor.newPage()
  await anonymous.goto(ids.shareLink ?? '')
  const shared = await boot(anonymous)
  expect(shared.kind === 'page' ? shared.snapshot.incidents.map((incident) => incident.title) : []).toEqual(['Payments are slow'])
  await visitor.close()

  // The page keeps auto-incidents as drafts; triage publishes them.
  await page.goto(`/n/${ids.node}/monitors/${ids.monitor}`)
  await page.getByRole('radiogroup', { name: 'Set status' }).getByRole('radio', { name: 'Down' }).click()
  await page.goto(`/n/${ids.node}/incidents`)
  const auto = page.locator('[data-incident="Checkout is down"]')
  await expect(auto).toBeVisible(STATUS_WAIT)
  await auto.getByRole('link').click()
  await expect(page).toHaveURL(/\/incidents\/\d+$/)
  ids.auto = new URL(page.url()).pathname.split('/')[4] ?? ''
  await expect(pages.locator(`[data-publication="${slug}"]`)).toHaveAttribute('data-published', 'false')
  await page.getByTestId('auto-incident').getByRole('button', { name: 'Publish to 1 page' }).click()
  await expect(pages.locator(`[data-publication="${slug}"]`)).toHaveAttribute('data-published', 'true')
  await expect(page.getByTestId('auto-incident').getByRole('button', { name: /Publish/ })).toHaveCount(0)
  await page.context().close()
})

test('a revision conflict keeps the layout draft until reloaded', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(editor('layout'))
  await page.getByRole('textbox', { name: 'Group name' }).fill('Everything')

  const other = await asAdmin(browser)
  await other.goto(editor('layout'))
  await other.getByRole('button', { name: 'Add group' }).click()
  await other.getByRole('button', { name: 'Save components' }).click()
  await expect(other.getByText('2 components on the page')).toBeVisible()
  await other.context().close()

  await expect(page.getByText('Someone changed this page since you started.')).toBeVisible()
  await page.getByRole('button', { name: 'Save components' }).click()
  await expect(page.getByText('Someone changed this since you opened it.', { exact: false })).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'Group name' })).toHaveValue('Everything')
  await page.getByRole('button', { name: 'Reload' }).click()
  await expect(page.getByRole('textbox', { name: 'Group name' })).toHaveCount(2)
  await expect(page.getByRole('textbox', { name: 'Group name' }).first()).toHaveValue('Services')
  await page.context().close()
})

test('page screens hold up at desktop and phone sizes, light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `admin-${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.node}/pages`)
      await expect(page.locator(`[data-page="${slug}"]`)).toBeVisible()
      await check('pages')

      await page.goto(editor())
      await expect(page.getByRole('img', { name: 'Page logo' })).toBeVisible()
      await check('page-settings')

      await page.goto(editor('layout'))
      await expect(page.locator('[data-component]')).toHaveCount(2)
      await check('page-layout')

      await page.goto(editor('access'))
      await expect(page.locator('[data-token="Client A"]')).toBeVisible()
      await check('page-access')

      await page.goto(editor('hostnames'))
      await expect(page.locator('[data-hostname="status.acme.test"]')).toBeVisible()
      await check('page-hostnames')

      await page.goto(editor('preview'))
      await expect(page.getByTestId('page-preview').locator('[data-preview-component="Support"]')).toBeVisible()
      await check('page-preview')

      await page.goto(`/n/${ids.node}/incidents/${ids.auto}`)
      await expect(page.getByRole('region', { name: 'Status pages' }).locator(`[data-publication="${slug}"]`)).toBeVisible()
      await check('incident-pages')
    }
    await page.context().close()
  }
})
