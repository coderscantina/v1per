import { expect, test } from '@playwright/test'

import { expectAccessible, ownNode } from './helpers'
import { sessionFile } from './session'

/**
 * The instance admin's Imports page against a fresh server: no Kuma import has run, so the list
 * says how to start one, and a node without an import is not available.
 */
test.use({ storageState: sessionFile })

test('an instance admin finds the imports page, empty on a fresh server', async ({ page }) => {
  await page.goto('/instance')
  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'Imports' }).click()
  await expect(page).toHaveURL(/\/instance\/imports$/)
  await expect(page.getByRole('heading', { level: 1, name: 'Imports' })).toBeVisible()
  await expect(page.getByText('No imports yet')).toBeVisible()
  await expect(page.getByText(/v1per import kuma --source kuma\.db --into <node>/)).toBeVisible()
  await expectAccessible(page)

  const { node } = await ownNode(page, 'Not imported')
  await page.goto(`/instance/imports/${node}`)
  await expect(page.getByText('This import is not available.')).toBeVisible()
  await page.getByRole('link', { name: 'All imports' }).click()
  await expect(page).toHaveURL(/\/instance\/imports$/)
})
