import { spawn, type ChildProcess } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { expect, test, type Browser, type Page } from '@playwright/test'

import { expectAccessible, expectNoHorizontalOverflow, schemes, shot, viewports } from './helpers'
import { sessionFile } from './session'

/**
 * Milestone 06 journeys against a real server and a real `v1per probe` agent the test starts:
 * an admin registers a probe, runs the agent with the command the dialog shows, assigns the
 * probe to an http monitor next to the built-in one and sees each probe's status; then revokes
 * the probe and sees the agent refused and stopped.
 */
test.describe.configure({ mode: 'serial', timeout: 180_000 })

const STATUS_WAIT = { timeout: 60_000 }
const ids: Record<string, string> = {}
let target: Server | null = null
let targetUrl = ''
let agent: ChildProcess | null = null
let agentData = ''

/** A plain HTTP endpoint that answers 200, standing in for a website. */
async function startTarget(): Promise<Server> {
  const server = createServer((_, response) => {
    response.writeHead(200, { 'content-type': 'text/plain' })
    response.end('ok')
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  return server
}

/** The agent's exit code, once it exits. */
function exited(child: ChildProcess): Promise<number | null> {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode)
  return new Promise((done) => child.once('exit', (code) => done(code)))
}

test.afterAll(async () => {
  // Only the agent this file started, by its recorded PID.
  const running = agent
  agent = null
  if (running?.pid !== undefined && running.exitCode === null) {
    const done = exited(running)
    process.kill(running.pid, 'SIGTERM')
    await done
    console.log(`stopped v1per probe, pid ${running.pid}`)
  }
  if (agentData) rmSync(agentData, { recursive: true, force: true })
  if (target) {
    target.closeAllConnections()
    await new Promise((done) => target?.close(done))
  }
})

async function asAdmin(browser: Browser, viewport: { width: number; height: number } = viewports.desktop) {
  const context = await browser.newContext({ viewport, storageState: sessionFile })
  return context.newPage()
}

const ownProbes = (page: Page) => page.getByRole('table', { name: 'Probes on this node' })
const probeRow = (page: Page, name: string) => ownProbes(page).locator(`tr[data-probe="${name}"]`)
const probeStates = (page: Page) => page.getByRole('table', { name: 'Probes', exact: true })

/** Opens the add dialog, names the probe and returns the command it shows. */
async function addProbe(page: Page, name: string): Promise<string> {
  await page.getByRole('button', { name: 'Add probe' }).first().click()
  const dialog = page.getByRole('dialog', { name: 'Add probe' })
  await dialog.getByLabel('Name', { exact: true }).fill(name)
  await dialog.getByRole('button', { name: 'Add probe' }).click()
  const created = page.getByRole('dialog', { name: 'Probe added' })
  await expect(created.getByText('Copy it now. It is shown only this once.')).toBeVisible()
  return created.getByLabel('Command').inputValue()
}

test('a registered probe runs a monitor next to the built-in one', async ({ browser }) => {
  const binary = process.env.V1PER_BIN
  if (!binary) throw new Error('global setup sets V1PER_BIN to the v1per binary')
  target = await startTarget()
  targetUrl = `http://127.0.0.1:${(target.address() as AddressInfo).port}/health`

  const page = await asAdmin(browser)
  await page.goto('/')
  await expect(page).toHaveURL(/\/n\/\d+$/)
  ids.root = new URL(page.url()).pathname.split('/')[2] ?? ''

  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'Probes' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Probes' })).toBeVisible()
  await expect(probeRow(page, 'Built-in').locator('[data-probe-status="online"]')).toBeVisible()

  const command = await addProbe(page, 'Frankfurt')
  const [, server, token] = command.match(/^v1per probe --server (\S+) --token (\S+)$/) ?? []
  expect(server).toBe(new URL(page.url()).origin)
  expect(token).toBeTruthy()
  await page.getByRole('dialog', { name: 'Probe added' }).getByRole('button', { name: 'Done' }).click()
  await expect(probeRow(page, 'Frankfurt').locator('[data-probe-status="waiting"]')).toBeVisible()

  agentData = mkdtempSync(join(tmpdir(), 'v1per-e2e-probe-'))
  agent = spawn(binary, ['probe', '--server', server ?? '', '--token', token ?? '', '--data-dir', agentData], {
    stdio: ['ignore', 'ignore', 'inherit'],
    env: { ...process.env, V1PER_LOG: 'warn' },
  })
  console.log(`started v1per probe, pid ${agent.pid}`)
  await expect(probeRow(page, 'Frankfurt').locator('[data-probe-status="online"]')).toBeVisible(STATUS_WAIT)
  await expect(probeRow(page, 'Frankfurt')).toContainText('On time')

  await page.goto(`/n/${ids.root}/monitors/new`)
  await page.getByLabel('Name', { exact: true }).fill('Edge check')
  await page.getByLabel('URL', { exact: true }).fill(targetUrl)
  await page.getByLabel('Interval in seconds', { exact: true }).fill('20')
  const picker = page.getByRole('group', { name: 'Probes' })
  await expect(picker.getByRole('checkbox', { name: 'Built-in' })).toBeChecked()
  await expect(page.getByRole('combobox', { name: 'Probe quorum' })).toBeHidden()
  await picker.getByRole('checkbox', { name: 'Frankfurt' }).check()
  await expect(page.getByRole('combobox', { name: 'Probe quorum' })).toHaveText(/Half or more/)
  await page.getByRole('button', { name: 'Add monitor' }).click()

  await expect(page).toHaveURL(/\/monitors\/\d+$/)
  ids.monitor = new URL(page.url()).pathname.split('/')[4] ?? ''
  const states = probeStates(page)
  await expect(states.locator('tr[data-probe="Frankfurt"] [data-status="up"]')).toBeVisible(STATUS_WAIT)
  await expect(states.locator('tr[data-probe="Built-in"] [data-status="up"]')).toBeVisible(STATUS_WAIT)
  await expect(states.locator('tr[data-probe="Frankfurt"] [data-live="true"]')).toBeVisible()

  // One probe's own checks.
  await page.getByRole('combobox', { name: 'Probe', exact: true }).click()
  await page.getByRole('option', { name: 'Frankfurt' }).click()
  await expect(page.locator('section[aria-labelledby="monitor-heartbeats"] header')).toContainText('Frankfurt')
  await expect(page.getByRole('table', { name: 'Checks' }).locator('[data-status="up"]').first()).toBeVisible(STATUS_WAIT)
  await page.context().close()
})

test('probe screens hold up at desktop and phone sizes, light and dark', async ({ browser }) => {
  for (const [size, viewport] of Object.entries(viewports)) {
    const page = await asAdmin(browser, viewport)
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme })
      const check = async (name: string) => {
        await expectNoHorizontalOverflow(page)
        await expectAccessible(page)
        await shot(page, `${name}-${size}-${scheme}`)
      }

      await page.goto(`/n/${ids.root}/probes`)
      await expect(probeRow(page, 'Frankfurt').locator('[data-probe-status="online"]')).toBeVisible()
      await check('probes')

      await addProbe(page, `Spare ${size} ${scheme}`)
      await check('probes-token')
      await page.getByRole('dialog', { name: 'Probe added' }).getByRole('button', { name: 'Done' }).click()

      await page.goto(`/n/${ids.root}/monitors/${ids.monitor}/edit`)
      // At the top, so the save button does not sit under the phone's translucent tab bar.
      await page.getByRole('group', { name: 'Probes' }).evaluate((element) => element.scrollIntoView({ block: 'start' }))
      await expect(page.getByRole('group', { name: 'Probes' }).getByRole('checkbox', { name: 'Frankfurt' })).toBeChecked()
      await check('editor-probes')

      await page.goto(`/n/${ids.root}/monitors/${ids.monitor}`)
      await expect(probeStates(page).locator('tr[data-probe="Frankfurt"] [data-status="up"]')).toBeVisible()
      await probeStates(page).evaluate((element) => element.scrollIntoView({ block: 'center' }))
      await check('detail-probes')
    }
    await page.context().close()
  }
})

test('a revoked probe is refused, its agent stops and it leaves the monitor', async ({ browser }) => {
  const running = agent
  if (!running) throw new Error('the agent from the first test is not running')
  const page = await asAdmin(browser)
  await page.goto(`/n/${ids.root}/probes`)
  await expect(probeRow(page, 'Frankfurt').locator('[data-probe-status="online"]')).toBeVisible()

  await page.getByRole('button', { name: 'Revoke Frankfurt' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Revoke' }).click()
  await expect(page.getByText('Frankfurt revoked.')).toBeVisible()
  await expect(probeRow(page, 'Frankfurt').locator('[data-probe-status="revoked"]')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Revoke Frankfurt' })).toHaveCount(0)

  // The agent's next request is refused; it exits with status 3.
  expect(await exited(running)).toBe(3)
  agent = null

  await page.goto(`/n/${ids.root}/monitors/${ids.monitor}`)
  await expect(probeStates(page).locator('tr[data-probe="Built-in"]')).toBeVisible()
  await expect(probeStates(page).locator('tr[data-probe="Frankfurt"]')).toHaveCount(0)
  await page.context().close()
})
