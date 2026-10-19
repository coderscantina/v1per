import { expect, test } from '@playwright/test'

import { expectAccessible, expectNoHorizontalOverflow, schemes, shot, viewports } from './helpers'
import { admin, instanceName } from './session'

/**
 * Signing in through the form. Each journey here spends sign-in attempts, and the server allows
 * 10 per address and minute, so the rest of the suite reuses the session from global setup.
 */

test('the sign-in page reads well on desktop and phone in both modes', async ({ page }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    await page.setViewportSize(viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      await page.goto('/login')
      await expect(page.getByRole('heading', { name: new RegExp(`Sign in\\s+to ${instanceName}`) })).toBeVisible()
      await expect(page.getByLabel('Email')).toBeFocused()
      await expectNoHorizontalOverflow(page)
      await expectAccessible(page)
      await shot(page, `login-${size}-${scheme}`)
    }
  }
})

test('sign-in refuses a wrong password, then signs in and returns to the page asked for', async ({ page }) => {
  await page.goto('/somewhere/else')
  await expect(page).toHaveURL((url) => url.pathname === '/login' && url.searchParams.get('redirect') === '/somewhere/else')

  for (const scheme of schemes) {
    await page.emulateMedia({ colorScheme: scheme })
    await page.getByLabel('Email').fill(admin.email)
    await page.getByLabel('Password').fill('not the password')
    await page.getByRole('button', { name: 'Sign in' }).click()
    await expect(page.getByText('Email or password is wrong.')).toBeVisible()
    await expectAccessible(page)
    await shot(page, `login-error-${scheme}`)
  }

  await page.getByLabel('Password').fill(admin.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/somewhere\/else$/)
  await expect(page.getByText('Page not found')).toBeVisible()
  await expect(page.getByRole('button', { name: /Account menu$/ })).toBeVisible()
})

test('signing out ends the session', async ({ page }) => {
  await page.goto('/login')
  await page.getByLabel('Email').fill(admin.email)
  await page.getByLabel('Password').fill(admin.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Root' })).toBeVisible()

  await page.getByRole('button', { name: /Account menu$/ }).click()
  await page.getByRole('menuitem', { name: 'Sign out' }).click()
  await expect(page).toHaveURL(/\/login$/)

  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)
})
