import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { randomBytes } from 'node:crypto'

import { chromium, type FullConfig } from '@playwright/test'

import { admin, instanceName, sessionFile } from './session'

const web = resolve(import.meta.dirname, '..')
const repo = resolve(web, '../..')
/** The built assets: `V1PER_E2E_DIST`, or `dist`. A separate directory lets suites run side by side. */
const dist = process.env.V1PER_E2E_DIST ?? join(web, 'dist')

/**
 * The `v1per` binary: `V1PER_BIN`, or a debug build of the workspace, built here with the
 * `v1per-probe` agent beside it if needed.
 */
function serverBinary(): string {
  if (process.env.V1PER_BIN) return process.env.V1PER_BIN
  const built = spawnSync('cargo', ['build', '-p', 'v1per-server', '-p', 'v1per-probe'], { cwd: repo, stdio: 'inherit' })
  if (built.status !== 0) throw new Error('cargo build -p v1per-server -p v1per-probe failed')
  return join(process.env.CARGO_TARGET_DIR ?? join(repo, 'target'), 'debug', 'v1per')
}

async function waitForHealth(baseURL: string, server: { exitCode: number | null }) {
  const deadline = Date.now() + 30_000
  while (Date.now() < deadline) {
    if (server.exitCode !== null) throw new Error(`v1per serve exited with ${server.exitCode}`)
    const healthy = await fetch(`${baseURL}/health`).then((r) => r.ok).catch(() => false)
    if (healthy) return
    await new Promise((done) => setTimeout(done, 200))
  }
  throw new Error(`v1per serve did not answer on ${baseURL} within 30 s`)
}

/**
 * Starts `v1per serve` on a fresh data directory with a known admin password, unless
 * `V1PER_E2E_URL` points at a running server. Then signs in once and stores the session, so the
 * suite stays under the server's 10 sign-ins per address and minute. Returns the teardown,
 * which stops only the server started here, by its recorded PID.
 */
export default async function globalSetup(config: FullConfig) {
  const baseURL = config.projects[0]?.use.baseURL
  if (!baseURL) throw new Error('playwright.config.ts sets no baseURL')
  let teardown = async () => {}

  if (!process.env.V1PER_E2E_URL) {
    if (!existsSync(join(dist, 'index.html'))) throw new Error(`${dist} is missing: run \`bun run build\` first`)
    const binary = serverBinary()
    process.env.V1PER_BIN = binary
    // Probe journeys start agents: `V1PER_PROBE_BIN`, or the one beside the server binary.
    process.env.V1PER_PROBE_BIN ??= join(dirname(binary), 'v1per-probe')
    const scratch = mkdtempSync(join(tmpdir(), 'v1per-e2e-'))
    const data = join(scratch, 'data')
    const passwordFile = join(scratch, 'password')
    const password = randomBytes(18).toString('base64url')
    writeFileSync(passwordFile, password, { mode: 0o600 })
    const init = spawnSync(
      binary,
      ['init', '--data-dir', data, '--admin-email', admin.email, '--admin-password-file', passwordFile],
      { stdio: 'inherit' }
    )
    if (init.status !== 0) throw new Error(`v1per init exited with ${init.status}`)

    const { host } = new URL(baseURL)
    const server = spawn(
      binary,
      ['serve', '--data-dir', data, '--bind', host, '--web-dir', dist, '--instance-name', instanceName],
      { stdio: ['ignore', 'ignore', 'inherit'], env: { ...process.env, V1PER_LOG: 'warn' } }
    )
    const pid = server.pid
    if (pid === undefined) throw new Error('v1per serve did not start')
    console.log(`started v1per serve, pid ${pid}, on ${baseURL}`)
    teardown = async () => {
      if (server.exitCode === null) {
        const exited = new Promise((done) => server.once('exit', done))
        process.kill(pid, 'SIGTERM')
        await exited
      }
      rmSync(scratch, { recursive: true, force: true })
      console.log(`stopped v1per serve, pid ${pid}`)
    }
    try {
      await waitForHealth(baseURL, server)
    } catch (error) {
      await teardown()
      throw error
    }
    process.env.V1PER_E2E_PASSWORD = password
  }

  try {
    await signIn(baseURL)
  } catch (error) {
    await teardown()
    throw error
  }
  return teardown
}

async function signIn(baseURL: string) {
  if (admin.password === '') throw new Error('set V1PER_E2E_PASSWORD for the server at V1PER_E2E_URL')
  const browser = await chromium.launch()
  try {
    const context = await browser.newContext({ baseURL })
    const page = await context.newPage()
    await page.goto('/login')
    await page.getByLabel('Email').fill(admin.email)
    await page.getByLabel('Password').fill(admin.password)
    await page.getByRole('button', { name: 'Sign in' }).click()
    await page.waitForURL((url) => !url.pathname.startsWith('/login'))
    mkdirSync(dirname(sessionFile), { recursive: true })
    await context.storageState({ path: sessionFile })
  } finally {
    await browser.close()
  }
}
