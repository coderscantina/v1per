import { expect, test, type Browser, type Page } from '@playwright/test'

import { expectAccessible, expectNoHorizontalOverflow, schemes, settle, shot, totp, viewports } from './helpers'
import { admin, sessionFile } from './session'

/**
 * Milestone 04 journeys against a real server: the admin builds `Root > Client services >
 * Autohaus > Schmidt | Motormobile` beside `Operations`, invites a client contact to Autohaus,
 * and that person, in a browser of their own, sees only their subtree. The tests share the tree
 * and the invitee, so they run in order.
 */
test.describe.configure({ mode: 'serial', timeout: 120_000 })

const invitee = { name: 'Clara Client', email: 'clara@example.test', password: 'clara-has-a-long-password' }

const ids: Record<string, string> = {}
let inviteeState: Awaited<ReturnType<Awaited<ReturnType<Browser['newContext']>>['storageState']>> | null = null

const nodeId = (page: Page) => new URL(page.url()).pathname.split('/')[2] ?? ''

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

async function addChild(page: Page, name: string) {
  await page.getByRole('button', { name: 'Add child node' }).click()
  const dialog = page.getByRole('dialog', { name: 'Add child node' })
  await dialog.getByLabel('Name').fill(name)
  await dialog.getByRole('button', { name: 'Add node' }).click()
  await expect(dialog).toBeHidden()
  const link = page.getByRole('region', { name: 'Child nodes' }).getByRole('link', { name })
  await expect(link).toBeVisible()
  ids[name] = (await link.getAttribute('href'))?.split('/')[2] ?? ''
}

async function open(page: Page, name: string) {
  await page.getByRole('region', { name: 'Child nodes' }).getByRole('link', { name }).click()
  await expect(page.getByRole('heading', { level: 1, name })).toBeVisible()
  ids[name] = nodeId(page)
}

test('an admin builds the tree and invites a client contact, who sees only their subtree', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto('/')
  await expect(page.getByRole('heading', { level: 1, name: 'Root' })).toBeVisible()
  await expect(page).toHaveURL(/\/n\/\d+$/)
  ids.Root = nodeId(page)

  await addChild(page, 'Client services')
  await addChild(page, 'Operations')
  await open(page, 'Operations')
  await page.goBack()
  await open(page, 'Client services')
  await addChild(page, 'Autohaus')
  await open(page, 'Autohaus')
  await addChild(page, 'Schmidt')
  await addChild(page, 'Motormobile')
  await shot(page, 'overview-desktop-light')

  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'Members' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Members' })).toBeVisible()
  await page.getByRole('button', { name: 'Invite' }).click()
  const dialog = page.getByRole('dialog', { name: 'Invite' })
  await dialog.getByLabel('Email').fill(invitee.email)
  await expect(dialog.getByRole('combobox', { name: 'Role' })).toHaveText(/Member/)
  await dialog.getByRole('button', { name: 'Invite' }).click()
  const link = await page.getByLabel('Invitation link').inputValue()
  expect(link).toMatch(/\/join\?code=/)
  await shot(page, 'invite-link-desktop-light')
  await page.getByRole('button', { name: 'Done' }).click()
  const pending = page.getByRole('table', { name: 'Invitations' }).getByRole('row', { name: new RegExp(invitee.email) })
  await expect(pending.getByText('Pending')).toBeVisible()

  // A second browser, signed out: the invitee creates an account from the link.
  const guest = await (await browser.newContext({ viewport: viewports.desktop })).newPage()
  await guest.goto(link)
  await expect(guest.getByText(/invited you to Autohaus\./)).toBeVisible()
  await expectAccessible(guest)
  await shot(guest, 'join-signup-desktop-light')
  await guest.getByLabel('Your name').fill(invitee.name)
  await guest.getByLabel('Password', { exact: true }).fill(invitee.password)
  await guest.getByLabel('Repeat password').fill(invitee.password)
  await guest.getByRole('button', { name: 'Create account and accept' }).click()
  await expect(guest.getByRole('heading', { level: 1, name: 'Autohaus' })).toBeVisible()
  expect(nodeId(guest)).toBe(ids.Autohaus)

  // The admin's open members page picks the new member up from the node topic.
  await expect(page.getByRole('table', { name: 'People with access' }).getByText(invitee.name)).toBeVisible()
  await expect(pending.getByText('Used')).toBeVisible()

  // The picker shows the invitee's subtree, hidden ancestors as placeholders, and nothing else.
  await guest.getByRole('button', { name: /^Node: Hidden node \/ Hidden node \/ Autohaus/ }).click()
  const options = guest.getByRole('listbox', { name: 'Nodes' }).getByRole('option')
  await expect(options).toHaveText(['Hidden node', 'Hidden node', 'Autohaus', 'Schmidt', 'Motormobile'])
  await expect(options.filter({ hasText: 'Hidden node' }).first()).toHaveAttribute('aria-disabled', 'true')
  await guest.keyboard.type('motor')
  await expect(options).toHaveText([/Motormobile/])
  await shot(guest, 'picker-search-desktop-light')
  await guest.keyboard.press('Enter')
  await expect(guest.getByRole('heading', { level: 1, name: 'Motormobile' })).toBeVisible()

  // Nodes outside the grant answer like nodes that do not exist.
  await guest.goto(`/n/${ids.Operations}`)
  await expect(guest.getByText('This node is not available')).toBeVisible()
  await guest.goto(`/n/${ids['Client services']}/members`)
  await expect(guest.getByText(/does not exist, or your account has no access/)).toBeVisible()

  // A member reads the members list but gets no controls.
  await guest.goto(`/n/${ids.Autohaus}/members`)
  await expect(guest.getByText(/Your role shows who has access/)).toBeVisible()
  await expect(guest.getByRole('button', { name: 'Invite' })).toHaveCount(0)
  await expect(guest.getByRole('button', { name: 'Leave' })).toBeVisible()
  await expect(guest.getByRole('combobox')).toHaveCount(0)

  inviteeState = await guest.context().storageState()
  await guest.context().close()
  await page.context().close()
})

test('the invitee sets up 2FA, then signs in with a code', async ({ browser }) => {
  test.skip(!inviteeState, 'needs the invitee from the first journey')
  const context = await browser.newContext({ viewport: viewports.desktop, storageState: inviteeState ?? undefined })
  const page = await context.newPage()
  await page.goto('/account/security')
  await expect(page.getByRole('heading', { name: 'Two-factor authentication' })).toBeVisible()
  await page.getByRole('button', { name: 'Set up' }).click()
  await page.getByRole('button', { name: 'Continue' }).click()
  const key = await page.getByLabel('Key for manual entry').inputValue()
  await expect(page.getByRole('img', { name: 'QR code for your authenticator app' })).toBeVisible()
  await expectAccessible(page)
  await shot(page, 'two-factor-setup-desktop-light')
  const enrolled = totp(key)
  await page.getByLabel('Code', { exact: true }).fill(enrolled.code)
  await page.getByRole('button', { name: 'Turn on' }).click()
  await expect(page.getByRole('list', { name: 'Recovery codes' }).getByRole('listitem')).toHaveCount(10)
  await shot(page, 'recovery-codes-desktop-light')
  await page.getByLabel('I have kept the recovery codes').check()
  await page.getByRole('button', { name: 'Done' }).click()
  await expect(page.getByText(/Recovery codes left|recovery codes left/)).toBeVisible()

  await page.getByRole('button', { name: /Account menu$/ }).click()
  await page.getByRole('menuitem', { name: 'Sign out' }).click()
  await page.getByLabel('Email').fill(invitee.email)
  await page.getByLabel('Password').fill(invitee.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/login\/verify/)
  await expect(page.getByRole('heading', { name: 'Two-factor check' })).toBeVisible()
  await expectAccessible(page)
  await shot(page, 'login-code-desktop-light')

  // A code counts once: wait for the next one if the window has not moved since enrollment.
  let next = totp(key)
  if (next.step === enrolled.step) {
    await page.waitForTimeout((next.step + 1) * 30_000 - Date.now() + 500)
    next = totp(key)
  }
  await page.getByLabel('Code').fill(next.code)
  await page.getByRole('button', { name: 'Verify' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Autohaus' }), 'the last node visited').toBeVisible()
  await context.close()
})

test('an API key is created with a password check and shown once', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.Autohaus}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Autohaus' })).toBeVisible()
  await page.goto('/account/api-keys')
  await page.getByRole('button', { name: 'Create key' }).click()
  const dialog = page.getByRole('dialog', { name: 'Create key' })
  await dialog.getByLabel('Name', { exact: true }).fill('Deploy pipeline')
  await expect(dialog.getByText(/reaches Root \/ Client services \/ Autohaus/)).toBeVisible()
  await dialog.getByLabel('Rename and add child nodes').check()
  await expect(dialog.getByLabel('Manage members')).toHaveCount(0)
  await expectAccessible(page)
  await shot(page, 'api-key-create-desktop-light')
  await dialog.getByRole('button', { name: 'Create key' }).click()

  const reauth = page.getByRole('dialog', { name: 'Confirm it is you' })
  await reauth.getByLabel('Password').fill(admin.password)
  await reauth.getByRole('button', { name: 'Confirm' }).click()
  const secret = await page.getByLabel('Key', { exact: true }).inputValue()
  expect(secret.length).toBeGreaterThan(20)
  await shot(page, 'api-key-secret-desktop-light')
  await page.getByRole('button', { name: 'Done' }).click()
  const row = page.getByRole('row', { name: /Deploy pipeline/ })
  await expect(row.getByText('Root / Client services / Autohaus')).toBeVisible()
  await expect(row.getByText('Rename and add child nodes')).toBeVisible()
  await page.context().close()
})

test('moving a node shows who gains or loses access first', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.Schmidt}/settings`)
  await page.getByRole('button', { name: 'Move node' }).click()
  const dialog = page.getByRole('dialog', { name: 'Move Schmidt' })
  const targets = dialog.getByRole('listbox', { name: 'New parent' })
  await expect(targets.getByRole('option', { name: /Autohaus/ })).toHaveAttribute('aria-disabled', 'true')
  await expect(targets.getByRole('option', { name: /Schmidt/ })).toHaveAttribute('aria-disabled', 'true')
  await targets.getByRole('option', { name: 'Operations' }).click()
  await expect(dialog.getByText('Lose access (1)')).toBeVisible()
  await expect(dialog.getByText(invitee.name)).toBeVisible()
  await expectAccessible(page)
  await shot(page, 'move-preview-desktop-light')
  await dialog.getByRole('button', { name: 'Back' }).click()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await page.context().close()
})

test('node screens at desktop and phone width in light and dark', async ({ browser }) => {
  // A second, pending invitation, so the join page can be shown without using it up.
  const inviter = await asAdmin(browser)
  await inviter.goto(`/n/${ids.Schmidt}/members`)
  await inviter.getByRole('button', { name: 'Invite' }).click()
  await inviter.getByRole('dialog', { name: 'Invite' }).getByLabel('Email').fill('dana@example.test')
  await inviter.getByRole('dialog', { name: 'Invite' }).getByRole('button', { name: 'Invite' }).click()
  const link = await inviter.getByLabel('Invitation link').inputValue()
  await inviter.context().close()

  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    const guest = await (await browser.newContext({ viewport })).newPage()
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      await guest.emulateMedia({ colorScheme: scheme })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.Autohaus}`)
      await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
      await check('overview')

      await page.getByRole('button', { name: /^Node: / }).click()
      await expect(page.getByRole('listbox', { name: 'Nodes' })).toBeVisible()
      await expectAccessible(page)
      await shot(page, `picker-${size}-${scheme}`)
      await page.keyboard.press('Escape')

      await page.goto(`/n/${ids.Autohaus}/members`)
      await expect(page.getByRole('table', { name: 'People with access' }).getByText(invitee.name)).toBeVisible()
      await check('members')

      await page.getByRole('button', { name: 'Invite' }).click()
      await expect(page.getByRole('dialog', { name: 'Invite' })).toBeVisible()
      await settle(page)
      await expectAccessible(page)
      await shot(page, `invite-${size}-${scheme}`)
      await page.keyboard.press('Escape')

      await page.goto(`/n/${ids.Autohaus}/settings`)
      await expect(page.getByRole('heading', { name: 'Place in the tree' })).toBeVisible()
      await check('settings')

      await page.goto('/account/security')
      await expect(page.getByText(/^Authenticator app/)).toBeVisible()
      await check('security')

      await page.goto('/account/api-keys')
      await expect(page.getByRole('row', { name: /Deploy pipeline/ })).toBeVisible()
      await check('api-keys')

      await page.goto('/instance')
      await expect(page.getByLabel('Public URL')).toBeVisible()
      await check('instance')

      await guest.goto(link)
      await expect(guest.getByText(/invited you to Schmidt\./)).toBeVisible()
      await expectNoHorizontalOverflow(guest)
      await expectAccessible(guest)
      await shot(guest, `join-${size}-${scheme}`)
    }
    await guest.context().close()
    await page.context().close()
  }
})
