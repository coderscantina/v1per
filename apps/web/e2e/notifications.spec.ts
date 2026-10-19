import { expect, test, type Browser, type Page } from '@playwright/test'

import { decodeWords, header, startDiscordStub, startSmtpSink, startTarget } from './fakes'
import { expectAccessible, expectNoHorizontalOverflow, ownNode, schemes, shot, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 07 journeys against a real server, a local SMTP sink and a Discord webhook stub the
 * test starts: an instance admin sets the instance mail server and sends a test email, adds an
 * email and a Discord channel, links both to an http monitor, stops its target and sees the
 * down alert in the sink, the stub and both delivery logs.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const ALERT_WAIT = { timeout: 90_000 }
const ids: Record<string, string> = {}
let smtp: Awaited<ReturnType<typeof startSmtpSink>>
let discord: Awaited<ReturnType<typeof startDiscordStub>>
let target: Awaited<ReturnType<typeof startTarget>>

test.beforeAll(async () => {
  smtp = await startSmtpSink()
  discord = await startDiscordStub()
  target = await startTarget()
})

test.afterAll(async () => {
  await target?.stop()
  await discord?.stop()
  await smtp?.stop()
})

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

async function choose(page: Page, label: string, option: string) {
  await page.getByRole('combobox', { name: label, exact: true }).click()
  await page.getByRole('option', { name: option, exact: true }).click()
}

const subjects = () => smtp.mails.map((mail) => decodeWords(header(mail, 'Subject') ?? ''))

test('the instance mail server sends a test email', async ({ browser }) => {
  const page = await asAdmin(browser)
  ids.node = (await ownNode(page, 'Alerting')).node

  await page.goto('/instance')
  const notifications = page.getByRole('region', { name: 'Notifications' })
  await notifications.getByLabel('Timezone').fill('Europe/Vienna')
  await notifications.getByLabel('Certificate warnings').fill('7, 30')
  await notifications.getByRole('button', { name: 'Save' }).click()
  await expect(notifications.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible()
  await page.reload()
  await expect(notifications.getByLabel('Certificate warnings')).toHaveValue('7, 30')
  await expect(page.getByLabel('Public URL')).toHaveValue('')

  const section = page.getByRole('region', { name: 'Mail server' })
  await expect(section.getByText('No mail server is set, so email channels need their own.')).toBeVisible()
  await section.getByLabel('SMTP host').fill('127.0.0.1')
  await section.getByLabel('Port').fill(String(smtp.port))
  await section.getByLabel('Username').fill('alerts')
  await section.getByLabel('Password').fill('sink-password')
  await section.getByLabel('From', { exact: true }).fill('v1per <alerts@example.test>')
  await section.getByRole('button', { name: 'Save' }).click()
  await expect(section.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible()
  await expect(section.getByText('Set', { exact: true })).toBeVisible()

  const test = page.getByRole('region', { name: 'Test email' })
  await test.getByLabel('Send to').fill('admin@example.test')
  await test.getByRole('button', { name: 'Send test email' }).click()
  await expect(test.getByText('Test message sent.')).toBeVisible({ timeout: 30_000 })
  expect(smtp.mails.at(-1)?.to).toEqual(['<admin@example.test>'])
  expect(subjects().at(-1)).toBe('Rockhouse monitoring Testing')
  await page.context().close()
})

test('an email channel with a subject template sends a test through the instance server', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/channels`)
  await expect(page.getByRole('heading', { level: 1, name: 'Notifications' })).toBeVisible()
  await expect(page.getByText('No notification channels yet')).toBeVisible()

  await page.getByRole('link', { name: 'Add channel' }).first().click()
  await expect(page.getByRole('heading', { level: 1, name: 'Add channel' })).toBeVisible()
  await expect(page.getByRole('combobox', { name: 'Provider' })).toHaveText(/Email/)

  // Nothing is sent until the form has a name and a recipient.
  await page.getByRole('button', { name: 'Add channel' }).click()
  await expect(page.getByLabel('Name', { exact: true })).toHaveAttribute('aria-invalid', 'true')
  await expect(page.getByLabel('To', { exact: true })).toHaveAttribute('aria-invalid', 'true')

  await page.getByLabel('Name', { exact: true }).fill('Ops mail')
  await page.getByRole('switch', { name: 'Default channel' }).click()
  await page.getByLabel('To', { exact: true }).fill('ops@example.test')
  await page.getByLabel('Subject template').fill('{{ name }}: {{ status }}')
  await page.getByText('Template variables').click()
  await expect(page.getByText('The address the monitor checks')).toBeVisible()
  await shot(page, 'channel-editor-email-desktop-light')

  await page.getByRole('button', { name: 'Send test' }).click()
  await expect(page.getByText('Test message sent.')).toBeVisible({ timeout: 30_000 })
  expect(smtp.mails.at(-1)?.to).toEqual(['<ops@example.test>'])
  expect(subjects().at(-1)).toBe('Monitor Name not available: ⚠️ Test')

  await page.getByRole('button', { name: 'Add channel' }).click()
  await expect(page).toHaveURL(/\/channels\/\d+$/)
  ids.email = new URL(page.url()).pathname.split('/')[4] ?? ''
  await expect(page.getByRole('heading', { level: 1, name: 'Ops mail' })).toBeVisible()
  // A test of the saved channel lands in its log, live.
  const log = page.getByRole('table', { name: 'Delivery log' })
  await expect(log.getByText('Nothing sent yet.')).toBeVisible()
  await page.getByRole('button', { name: 'Send test' }).click()
  await expect(page.getByText('Test message sent.')).toBeVisible({ timeout: 30_000 })
  await expect(log.locator('[data-delivery-row="delivered"]').filter({ hasText: 'Ops mail Testing' })).toBeVisible()
  await page.context().close()
})

test('a Discord channel posts a test to its webhook', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/channels/new`)
  await choose(page, 'Provider', 'Discord')
  await page.getByLabel('Name', { exact: true }).fill('Ops Discord')
  await page.getByLabel('Webhook URL').fill(discord.url)
  await page.getByLabel('Display name').fill('v1per')
  await choose(page, 'Message format', 'Normal')
  await page.getByLabel('Prefix message').fill('@here')
  await shot(page, 'channel-editor-discord-desktop-light')

  await page.getByRole('button', { name: 'Add channel' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Ops Discord' })).toBeVisible()
  ids.discord = new URL(page.url()).pathname.split('/')[4] ?? ''
  await page.getByRole('button', { name: 'Send test' }).click()
  await expect(page.getByText('Test message sent.')).toBeVisible({ timeout: 30_000 })
  expect(discord.posts.at(-1)).toMatchObject({ username: 'v1per', content: 'Ops Discord Testing' })

  // The stored webhook stays sealed: the editor only says it is set.
  await page.getByRole('link', { name: 'Edit' }).click()
  await expect(page.getByText('Set', { exact: true })).toBeVisible()
  await expect(page.getByText(discord.url)).toHaveCount(0)

  await page.goto(`/n/${ids.node}/channels`)
  const table = page.getByRole('table', { name: 'Channels on this node' })
  await expect(table.locator('[data-channel="Ops mail"]')).toContainText('Default')
  await expect(table.locator('[data-channel="Ops Discord"]')).toContainText('Discord')
  await page.context().close()
})

test('a down monitor alerts both channels and shows in their delivery logs', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/monitors/new`)
  await page.getByLabel('Name', { exact: true }).fill('Web API')
  await page.getByLabel('URL', { exact: true }).fill(target.url)
  await page.getByLabel('Interval in seconds', { exact: true }).fill('20')
  const picker = page.getByRole('group', { name: 'Channels' })
  // A new monitor starts with the default channels.
  await expect(picker.getByRole('checkbox', { name: /Ops mail/ })).toBeChecked()
  await picker.getByRole('checkbox', { name: /Ops Discord/ }).click()
  await page.getByRole('switch', { name: 'Alert before the certificate expires' }).click()
  await page.getByRole('button', { name: 'Add monitor' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Web API' })).toBeVisible()
  ids.monitor = new URL(page.url()).pathname.split('/')[4] ?? ''
  await expect(page.getByRole('region', { name: 'Current status' }).locator('[data-status="up"]')).toBeVisible({ timeout: 60_000 })

  await target.stop()
  await expect(page.getByRole('region', { name: 'Current status' }).locator('[data-status="down"]')).toBeVisible({ timeout: 60_000 })

  await expect.poll(() => subjects().some((subject) => subject === 'Web API: 🔴 Down'), ALERT_WAIT).toBe(true)
  await expect
    .poll(
      () =>
        discord.posts.some((post) =>
          JSON.stringify(post).includes('❌ Your service Web API went down. ❌')
        ),
      ALERT_WAIT
    )
    .toBe(true)
  expect(discord.posts.find((post) => JSON.stringify(post).includes('went down'))).toMatchObject({ content: '@here' })

  const alerts = page.getByRole('table', { name: 'Recent alerts' })
  await expect(alerts.locator('[data-delivery-row="delivered"]').filter({ hasText: '[Web API] [🔴 Down]' })).toHaveCount(2, ALERT_WAIT)

  await page.goto(`/n/${ids.node}/channels/${ids.discord}`)
  const log = page.getByRole('table', { name: 'Delivery log' })
  await expect(log.locator('[data-delivery-row="delivered"]').filter({ hasText: '[Web API] [🔴 Down]' })).toBeVisible()

  // The monitor editor shows what was picked.
  await page.goto(`/n/${ids.node}/monitors/${ids.monitor}/edit`)
  const saved = page.getByRole('group', { name: 'Channels' })
  await expect(saved.getByRole('checkbox', { name: /Ops mail/ })).toBeChecked()
  await expect(saved.getByRole('checkbox', { name: /Ops Discord/ })).toBeChecked()
  await expect(page.getByRole('switch', { name: 'Alert before the certificate expires' })).toBeChecked()
  await page.context().close()
})

test('notification screens hold up at desktop and phone sizes, light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.node}/channels`)
      await expect(page.locator('[data-channel="Ops mail"]')).toBeVisible()
      await check('channels')

      await page.goto(`/n/${ids.node}/channels/${ids.email}/edit`)
      await expect(page.getByLabel('Subject template')).toHaveValue('{{ name }}: {{ status }}')
      await page.getByText('Template variables').click()
      await page.getByLabel('Subject template').scrollIntoViewIfNeeded()
      await check('channel-editor-email')

      await page.goto(`/n/${ids.node}/channels/${ids.discord}/edit`)
      await expect(page.getByLabel('Display name')).toHaveValue('v1per')
      await page.getByRole('combobox', { name: 'Message format' }).scrollIntoViewIfNeeded()
      await check('channel-editor-discord')

      await page.goto(`/n/${ids.node}/channels/${ids.discord}`)
      await expect(page.getByRole('table', { name: 'Delivery log' }).locator('[data-delivery-row]').first()).toBeVisible()
      await page.getByRole('table', { name: 'Delivery log' }).scrollIntoViewIfNeeded()
      await check('delivery-log')

      await page.goto(`/n/${ids.node}/monitors/${ids.monitor}/edit`)
      await page.getByRole('group', { name: 'Channels' }).scrollIntoViewIfNeeded()
      await check('monitor-editor-channels')

      await page.goto('/instance')
      await page.getByRole('region', { name: 'Mail server' }).scrollIntoViewIfNeeded()
      await check('instance-mail')
    }
    await page.context().close()
  }
})
