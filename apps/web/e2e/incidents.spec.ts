import { expect, test, type Browser, type Page } from '@playwright/test'

import { expectAccessible, expectNoHorizontalOverflow, ownNode, schemes, shot, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 08 journeys against a real server: a manual monitor set down opens an
 * auto-incident; a member opens an incident on that monitor and a static component, posts an
 * update that changes an impact, and resolves it; then plans a weekday maintenance with a
 * preview of its windows and a manual one that puts the monitor into maintenance until paused.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const STATUS_WAIT = { timeout: 60_000 }
const ids: Record<string, string> = {}

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

async function choose(page: Page, label: string, option: string) {
  await page.getByRole('combobox', { name: label, exact: true }).click()
  await page.getByRole('option', { name: option, exact: true }).click()
}

const statusPanel = (page: Page) => page.getByRole('region', { name: 'Current status' })

test('a static component is added, and a manual monitor going down opens an auto-incident', async ({ browser }) => {
  const page = await asAdmin(browser)
  ids.node = (await ownNode(page, 'Incident desk')).node

  await page.goto(`/n/${ids.node}/components`)
  await page.getByRole('button', { name: 'Add component' }).first().click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Name').fill('Support')
  await dialog.getByLabel('Description').fill('Phone and email support')
  await dialog.getByRole('button', { name: 'Add component' }).click()
  await expect(page.locator('[data-component="Support"]')).toBeVisible()

  await page.goto(`/n/${ids.node}/monitors/new`)
  await choose(page, 'Type', 'Manual')
  await page.getByLabel('Name', { exact: true }).fill('Checkout')
  await page.getByLabel('Interval in seconds', { exact: true }).fill('20')
  await page.getByRole('button', { name: 'Add monitor' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Checkout' })).toBeVisible()
  ids.monitor = new URL(page.url()).pathname.split('/')[4] ?? ''
  await page.getByRole('radiogroup', { name: 'Set status' }).getByRole('radio', { name: 'Down' }).click()
  await expect(statusPanel(page).locator('[data-status="down"]')).toBeVisible(STATUS_WAIT)

  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'Incidents' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Incidents' })).toBeVisible()
  const auto = page.locator('[data-incident="Checkout is down"]')
  await expect(auto).toBeVisible(STATUS_WAIT)
  await expect(auto).toContainText('Auto-incident')
  await auto.getByRole('link').click()
  await expect(page.getByTestId('auto-incident')).toContainText('Opened when this monitor went down: Checkout.')
  await expect(page.getByTestId('auto-incident').getByRole('button', { name: 'Dismiss' })).toBeVisible()
  // No status page shows Checkout, so there is nothing to publish it to.
  await expect(page.getByRole('button', { name: /Publish/ })).toHaveCount(0)
  ids.auto = new URL(page.url()).pathname.split('/')[4] ?? ''
  await page.context().close()
})

test('an incident is opened, updated with a new impact and resolved', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/incidents`)
  await page.getByRole('link', { name: 'Open incident' }).first().click()
  await expect(page.getByRole('heading', { level: 1, name: 'Open incident' })).toBeVisible()

  await page.getByLabel('Title').fill('Payments are slow')
  await page.getByLabel('Message').fill('Card payments take up to a minute.')
  const affected = page.getByRole('group', { name: 'Affected' })
  await affected.locator('[data-target="Checkout"]').getByRole('checkbox').click()
  await affected.locator('[data-target="Support"]').getByRole('checkbox').click()
  await expect(affected.getByRole('combobox', { name: 'Impact on Checkout' })).toHaveText(/Major outage/)
  await affected.getByRole('combobox', { name: 'Impact on Support' }).click()
  await page.getByRole('option', { name: 'Partial outage' }).click()
  await page.getByRole('button', { name: 'Open incident' }).click()

  await expect(page.getByRole('heading', { level: 1, name: 'Payments are slow' })).toBeVisible()
  ids.incident = new URL(page.url()).pathname.split('/')[4] ?? ''
  const status = page.getByRole('region', { name: 'Status' })
  await expect(status.locator('[data-affected="Support"] [data-impact="partial_outage"]')).toBeVisible()
  await expect(status.locator('[data-affected="Checkout"] [data-impact="major_outage"]')).toBeVisible()
  const timeline = page.getByRole('region', { name: 'Timeline' })
  await expect(timeline.locator('[data-update]')).toHaveCount(1)

  const update = page.getByRole('region', { name: 'Post an update' })
  await update.getByRole('radio', { name: 'Identified' }).click()
  await update.getByLabel('Message').fill('The card provider is slow. We switched to the backup.')
  await update.getByRole('combobox', { name: 'Impact on Support' }).click()
  await page.getByRole('option', { name: 'Degraded' }).click()
  await update.getByRole('button', { name: 'Post update' }).click()
  await expect(timeline.locator('[data-update]')).toHaveCount(2)
  await expect(timeline.locator('[data-update]').first()).toContainText('The card provider is slow.')
  await expect(status.locator('[data-affected="Support"] [data-impact="degraded"]')).toBeVisible()
  // The latest update that names a component wins; the other keeps its impact.
  await expect(status.locator('[data-affected="Checkout"] [data-impact="major_outage"]')).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()

  await page.getByRole('button', { name: 'Resolve' }).click()
  await expect(status.locator('[data-state="resolved"]')).toBeVisible()
  await expect(status.locator('[data-affected="Support"] [data-impact="operational"]')).toBeVisible()
  await expect(timeline.locator('[data-update]')).toHaveCount(3)

  await page.goto(`/n/${ids.node}/incidents`)
  await expect(page.locator('[data-incident="Payments are slow"]')).toHaveCount(0)
  await page.getByRole('radio', { name: 'Resolved' }).click()
  await expect(page).toHaveURL(/status=resolved/)
  await expect(page.locator('[data-incident="Payments are slow"]')).toBeVisible()
  await page.context().close()
})

test('a weekday maintenance previews its next windows', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/maintenance`)
  await page.getByRole('link', { name: 'Plan maintenance' }).first().click()
  await expect(page.getByRole('heading', { level: 1, name: 'Plan maintenance' })).toBeVisible()

  await page.getByLabel('Title').fill('Patch window')
  await choose(page, 'Repeats', 'On weekdays')
  await expect(page.getByText('Fill in the schedule to see its windows.')).toBeVisible()
  await page.getByRole('group', { name: 'Weekdays' }).getByRole('checkbox', { name: 'Monday' }).click()
  await page.getByRole('group', { name: 'Weekdays' }).getByRole('checkbox', { name: 'Thursday' }).click()
  await page.getByLabel('Timezone').fill('Europe/Vienna')
  const preview = page.getByTestId('maintenance-preview')
  await expect(preview.locator('li')).toHaveCount(10)
  await expect(page.getByText('Times in Europe/Vienna')).toBeVisible()
  await expect(preview.locator('li').first()).toContainText(/2:00/)

  // A cron the server cannot read says so in the preview.
  await choose(page, 'Repeats', 'Cron expression')
  await page.getByRole('textbox', { name: 'Cron expression' }).fill('not a cron')
  await expect(page.getByText(/^Cron expression: /)).toBeVisible()
  await choose(page, 'Repeats', 'On weekdays')
  await expect(preview.locator('li')).toHaveCount(10)

  await page.getByRole('group', { name: 'Covered monitors' }).getByRole('checkbox', { name: /Checkout/ }).click()
  await page.getByRole('button', { name: 'Plan maintenance' }).click()
  await expect(page).toHaveURL(/\/maintenance$/)
  const row = page.locator('[data-maintenance-row="Patch window"]')
  await expect(row.locator('[data-maintenance="scheduled"]')).toBeVisible()
  ids.patch = (await row.getByRole('link', { name: 'Patch window', exact: true }).getAttribute('href'))?.split('/')[4] ?? ''
  await page.context().close()
})

test('a manual maintenance puts its monitor into maintenance until paused', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/maintenance/new`)
  await page.getByLabel('Title').fill('Database upgrade')
  await choose(page, 'Repeats', 'Manual')
  await expect(page.getByText('A manual maintenance has no windows. It is active until you pause it.')).toBeVisible()
  await page.getByRole('group', { name: 'Covered monitors' }).getByRole('checkbox', { name: /Checkout/ }).click()
  await page.getByRole('button', { name: 'Plan maintenance' }).click()

  const row = page.locator('[data-maintenance-row="Database upgrade"]')
  await expect(row.locator('[data-maintenance="under_maintenance"]')).toBeVisible()

  await page.goto(`/n/${ids.node}/monitors/${ids.monitor}`)
  await expect(statusPanel(page).locator('[data-status="maintenance"]')).toBeVisible(STATUS_WAIT)

  await page.goto(`/n/${ids.node}`)
  await expect(page.getByRole('region', { name: 'Maintenance' }).locator('[data-maintenance="under_maintenance"]')).toBeVisible()
  await expect(page.getByRole('region', { name: 'Open incidents' }).getByText('Checkout is down')).toBeVisible()

  await page.goto(`/n/${ids.node}/maintenance`)
  await row.getByRole('button', { name: 'Pause Database upgrade' }).click()
  await expect(row.locator('[data-maintenance="inactive"]')).toBeVisible()
  await page.goto(`/n/${ids.node}/monitors/${ids.monitor}`)
  await expect(statusPanel(page).locator('[data-status="down"]')).toBeVisible(STATUS_WAIT)
  await page.context().close()
})

test('incident and maintenance screens hold up at desktop and phone sizes, light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.node}/incidents?status=all`)
      await expect(page.locator('[data-incident="Payments are slow"]')).toBeVisible()
      await check('incidents')

      await page.goto(`/n/${ids.node}/incidents/${ids.incident}`)
      await expect(page.getByRole('region', { name: 'Timeline' }).locator('[data-update]')).toHaveCount(3)
      await check('incident-detail')

      await page.goto(`/n/${ids.node}/incidents/${ids.auto}`)
      await expect(page.getByTestId('auto-incident')).toBeVisible()
      await check('incident-auto')

      await page.goto(`/n/${ids.node}/incidents/new`)
      await page.getByRole('group', { name: 'Affected' }).locator('[data-target="Support"]').getByRole('checkbox').click()
      await page.getByRole('group', { name: 'Affected' }).scrollIntoViewIfNeeded()
      await check('incident-editor')

      await page.goto(`/n/${ids.node}/maintenance`)
      await expect(page.locator('[data-maintenance-row="Patch window"]')).toBeVisible()
      await check('maintenance')

      await page.goto(`/n/${ids.node}/maintenance/${ids.patch}/edit`)
      await expect(page.getByTestId('maintenance-preview').locator('li')).toHaveCount(10)
      if (size === 'phone') await page.getByTestId('maintenance-preview').scrollIntoViewIfNeeded()
      await check('maintenance-editor')

      await page.goto(`/n/${ids.node}/components`)
      await expect(page.locator('[data-component="Support"]')).toBeVisible()
      await check('components')

      await page.goto(`/n/${ids.node}`)
      await expect(page.getByRole('region', { name: 'Open incidents' })).toBeVisible()
      await check('overview')
    }
    await page.context().close()
  }
})

test('an auto-incident can be dismissed as a false alarm', async ({ browser }) => {
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.node}/incidents/${ids.auto}`)
  await page.getByTestId('auto-incident').getByRole('button', { name: 'Dismiss' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Dismiss' }).click()
  await expect(page).toHaveURL(new RegExp(`/n/${ids.node}/incidents$`))
  await expect(page.locator(`a[href$="/incidents/${ids.auto}"]`)).toHaveCount(0)
  await page.goto(`/n/${ids.node}/incidents/${ids.auto}`)
  await expect(page.getByText('This incident is not available.')).toBeVisible()
  await page.context().close()
})
