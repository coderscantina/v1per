import { join } from 'node:path'

import { expect, request, test, type APIRequestContext, type Page } from '@playwright/test'

import type { Commands } from '../src/generated/commands'
import type { InstanceSettings, PageSubscribers } from '../src/generated'
import { REQUEST_MARKER_HEADER, REQUEST_MARKER_VALUE } from '../src/generated/limits'
import { decodeWords, header, startSmtpSink, type Mail } from './fakes'
import { expectAccessible, expectNoHorizontalOverflow, schemes, settle, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Page subscriptions against a real server and a local mail sink: a visitor subscribes on a
 * public page, confirms from the mailed link, hears about an incident on the component they
 * follow, and unsubscribes. The instance gets a mail server and a public URL for the run, since
 * mailed links need both, and its settings back afterwards.
 */
test.describe.configure({ mode: 'serial', timeout: 120_000 })

const MAIL_WAIT = { timeout: 30_000 }
const ADDRESS = 'visitor@example.test'
const run = Date.now().toString(36)
const slug = `follow-${run}`
const ids = { root: '', website: '', shop: '', page: '' }
type CommandName = keyof Commands
let api: APIRequestContext
let smtp: Awaited<ReturnType<typeof startSmtpSink>>
let before: InstanceSettings

async function command<N extends CommandName>(name: N, args: Commands[N]['args']): Promise<Commands[N]['result']> {
  const response = await api.post(`/api/commands/${name}`, { data: { idempotency_key: crypto.randomUUID(), ...args } })
  expect(response.status(), `${name}: ${await response.text()}`).toBe(200)
  return (await response.json()) as Commands[N]['result']
}

const shots = process.env.V1PER_E2E_SHOTS

async function capture(page: Page, name: string) {
  if (!shots) return
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`) })
}

/** The text of a mail, with quoted-printable line breaks and escapes undone. */
const body = (mail: Mail) =>
  mail.data
    .replace(/=\r?\n/g, '')
    .replace(/=([0-9A-F]{2})/g, (_, hex: string) => String.fromCharCode(Number.parseInt(hex, 16)))

/** The newest mail to the visitor whose subject holds `subject`. */
async function mailAbout(subject: string): Promise<Mail> {
  const find = () =>
    smtp.mails.findLast((mail) => mail.to.includes(`<${ADDRESS}>`) && decodeWords(header(mail, 'Subject') ?? '').includes(subject))
  await expect.poll(() => find() !== undefined, MAIL_WAIT).toBe(true)
  const mail = find()
  if (!mail) throw new Error(`no mail about ${subject}`)
  return mail
}

/** The path and query of the subscription link in a mail, optionally the one with `marker`. */
function linkIn(mail: Mail, marker = ''): string {
  const links = body(mail).match(/https?:\/\/\S+\/subscription\?token=\S+/g) ?? []
  const link = links.find((entry) => entry.includes(marker) && (marker !== '' || !entry.includes('action=')))
  if (!link) throw new Error('no subscription link in the mail')
  const url = new URL(link)
  return `${url.pathname}${url.search}`
}

async function subscribers(): Promise<string[]> {
  const response = await api.get(`/api/queries/pages.subscribers?page=${ids.page}`)
  return ((await response.json()) as PageSubscribers).subscribers.map((subscriber) => subscriber.email)
}

test.beforeAll(async ({ baseURL }) => {
  if (!baseURL) throw new Error('no baseURL')
  smtp = await startSmtpSink()
  api = await request.newContext({
    baseURL,
    storageState: sessionFile,
    extraHTTPHeaders: { [REQUEST_MARKER_HEADER]: REQUEST_MARKER_VALUE, Origin: new URL(baseURL).origin },
  })
  before = (await (await api.get('/api/queries/instance.settings')).json()) as InstanceSettings
  await command('instance.updateSettings', { name: before.name, public_url: new URL(baseURL).origin })
  await command('instance.updateSmtp', { smtp: { host: '127.0.0.1', port: smtp.port, ignore_starttls: true, password: { set: false }, from: 'v1per <status@example.test>' },
  })

  const tree = await api.get('/api/queries/nodes.tree')
  const root = ((await tree.json()) as { nodes: { id: string; depth: number }[] }).nodes.find((node) => node.depth === 0)?.id
  if (!root) throw new Error('no root node')
  ids.root = root
  ids.website = (await command('components.create', { node: root, name: `Website ${run}`, description: '' })).component.id
  ids.shop = (await command('components.create', { node: root, name: `Shop ${run}`, description: '' })).component.id
  const created = await command('pages.create', { node: root, slug, title: 'Rockhouse status', locales: ['en', 'de'], subscriptions: true })
  ids.page = created.page.id
  const laid = await command('pages.updateLayout', {
    page: ids.page,
    expected_revision: created.page.revision,
    groups: [
      {
        name: '',
        components: [
          { target: { type: 'component', component: ids.website }, display_name: 'Website' },
          { target: { type: 'component', component: ids.shop }, display_name: 'Shop' },
        ],
      },
    ],
  })
  await command('pages.updateAccess', { page: ids.page, expected_revision: laid.page.revision, public: true })
})

test.afterAll(async () => {
  await command('instance.updateSettings', { name: before.name, public_url: before.public_url })
  await command('instance.updateSmtp', { smtp: before.smtp })
  await smtp?.stop()
  await api?.dispose()
})

test('the subscribe dialog offers email, feeds and the calendar', async ({ browser }) => {
  for (const [name, viewport] of Object.entries(viewports)) {
    for (const colorScheme of schemes) {
      const context = await browser.newContext({ viewport, colorScheme })
      const page = await context.newPage()
      await page.goto(`/s/${slug}`)
      await page.getByRole('button', { name: 'Subscribe' }).click()
      const dialog = page.getByRole('dialog', { name: 'Subscribe to updates' })
      await expect(dialog.getByLabel('Email')).toBeFocused()
      await dialog.getByLabel('Only some components').check()
      await expect(dialog.getByLabel('Website')).toBeVisible()
      await expect(dialog.getByRole('link', { name: 'Atom feed' })).toHaveAttribute('href', `/s/${slug}/feed.atom`)
      await expect(dialog.getByRole('link', { name: 'Maintenance calendar' })).toHaveAttribute('href', `/s/${slug}/maintenance.ics`)
      await expectAccessible(page)
      await expectNoHorizontalOverflow(page)
      await capture(page, `subscribe-dialog-${name}-${colorScheme}`)
      await page.keyboard.press('Escape')
      await expect(dialog).toBeHidden()
      await expect(page.getByRole('button', { name: 'Subscribe' })).toBeFocused()
      await context.close()
    }
  }
})

test('a visitor subscribes, confirms from the mail, hears about an incident and unsubscribes', async ({ browser }) => {
  const context = await browser.newContext({ viewport: viewports.desktop, colorScheme: 'dark' })
  const page = await context.newPage()
  await page.goto(`/s/${slug}`)
  await page.getByRole('button', { name: 'Subscribe' }).click()
  const dialog = page.getByRole('dialog', { name: 'Subscribe to updates' })
  await dialog.getByLabel('Email').fill(ADDRESS)
  await dialog.getByLabel('Only some components').check()
  await dialog.getByLabel('Website').check()
  await dialog.getByRole('button', { name: 'Subscribe by email' }).click()
  await expect(dialog.getByTestId('subscribe-sent')).toBeVisible()

  // Opening the mailed link confirms nothing; the visitor's click does.
  const confirmations: string[] = []
  page.on('request', (sent) => {
    if (sent.url().endsWith('/subscription/confirm')) confirmations.push(sent.url())
  })
  await page.goto(linkIn(await mailAbout('Confirm your subscription')))
  const view = page.getByTestId('subscription')
  await expect(view).toContainText(`Confirm that ${ADDRESS} should get updates from this page.`)
  await settle(page)
  expect(confirmations).toEqual([])
  expect(await subscribers()).toEqual([])
  await capture(page, 'subscription-confirm-desktop-dark')
  await view.getByRole('button', { name: 'Confirm subscription' }).click()
  await expect(view.getByTestId('subscription-note')).toHaveText('You are subscribed.')
  await expect(view.getByLabel('Website')).toBeChecked()
  await expect(view.getByLabel('Shop')).not.toBeChecked()
  await expectAccessible(page)
  await capture(page, 'subscription-manage-desktop-dark')
  expect(await subscribers()).toEqual([ADDRESS])

  const admin = await browser.newContext({ viewport: viewports.desktop, colorScheme: 'dark', storageState: sessionFile })
  const tab = await admin.newPage()
  await tab.goto(`/n/${ids.root}/pages/${ids.page}/subscribers`)
  await expect(tab.locator(`[data-subscriber="${ADDRESS}"]`)).toContainText('Website')
  await expectAccessible(tab)
  await capture(tab, 'admin-subscribers-desktop-dark')
  await admin.close()

  // An incident on the Shop is not theirs; one on the Website is.
  for (const [title, component] of [
    ['Checkout is slow', ids.shop],
    ['Website is down', ids.website],
  ] as const) {
    const opened = await command('incidents.create', {
      node: ids.root,
      title,
      message: 'We are looking into it.',
      impacts: [{ target: { type: 'component', component }, impact: 'major_outage' }],
    })
    await command('incidents.publish', { incident: opened.incident.id, page: ids.page })
  }
  const notice = await mailAbout('Website is down')
  expect(body(notice)).toContain('We are looking into it.')
  expect(header(notice, 'List-Unsubscribe')).toContain(`/s/${slug}/unsubscribe?token=`)
  expect(smtp.mails.some((mail) => decodeWords(header(mail, 'Subject') ?? '').includes('Checkout is slow'))).toBe(false)
  const feed = await (await api.get(`/s/${slug}/feed.atom`)).text()
  expect(feed).toContain('Website is down')

  await page.goto(linkIn(notice, 'action=unsubscribe'))
  await expect(view).toContainText(`Stop sending updates from this page to ${ADDRESS}?`)
  expect(await subscribers()).toEqual([ADDRESS])
  await view.getByRole('button', { name: 'Unsubscribe' }).click()
  await expect(view).toContainText('You are unsubscribed and your address is deleted.')
  expect(await subscribers()).toEqual([])
  await context.close()
})
