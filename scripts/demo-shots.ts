/**
 * Captures the README screenshots and the walkthrough GIF from the demo `scripts/demo-seed.ts`
 * runs. Start the seed, wait for its incident updates, then:
 *
 *   bun scripts/demo-shots.ts [--dir /tmp/v1per-demo]
 *
 * Writes `docs/assets/*.png` (1440 px wide at 2x, dark mode), `docs/assets/walkthrough.gif` and
 * `.github/social-preview.png` from `scripts/social-preview.html`.
 * Uses the web app's Playwright and `ffmpeg` for the GIF and PNG compression.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'

import { chromium, type Browser, type BrowserContextOptions, type Locator, type Page } from '../apps/web/node_modules/playwright'

import type { MonitorList, NodeTree, IncidentPage } from '../apps/web/src/generated'

const repo = resolve(import.meta.dir, '..')
const assets = join(repo, 'docs', 'assets')
const socialCard = join(repo, '.github', 'social-preview.png')
const { values: options } = parseArgs({ options: { dir: { type: 'string', default: '/tmp/v1per-demo' } } })
const demo = JSON.parse(readFileSync(join(resolve(options.dir), 'demo.json'), 'utf8')) as { url: string; email: string; password: string }

const desktop: BrowserContextOptions = {
  baseURL: demo.url,
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
  timezoneId: 'Europe/Vienna',
  locale: 'en-GB',
  colorScheme: 'dark',
}

/** Waits for fonts, two frames and every finite time-based animation, so a shot sees the settled page. */
async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready)
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => animation.timeline instanceof DocumentTimeline)
        .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
        .map((animation) => animation.finished.catch(() => undefined))
    )
  )
  await page.waitForTimeout(300)
}

/** Writes `<name>.png`, the viewport or the page down to the bottom of `until`. */
async function shot(page: Page, name: string, until?: Locator) {
  // The admin app scrolls inside its own panes; a click may have moved one.
  await page.evaluate(() => document.querySelectorAll('*').forEach((element) => element.scrollTo(0, 0)))
  await settle(page)
  const box = until ? await until.boundingBox() : null
  const clip = box ? { x: 0, y: 0, width: 1440, height: Math.ceil(box.y + box.height + 24) } : undefined
  await page.screenshot({ path: join(assets, `${name}.png`), clip, fullPage: Boolean(clip) })
  console.log(`wrote ${name}.png`)
}

async function signIn(browser: Browser): Promise<string> {
  const context = await browser.newContext(desktop)
  const page = await context.newPage()
  await page.goto('/login')
  await page.getByLabel('Email').fill(demo.email)
  await page.getByLabel('Password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await page.waitForURL((url) => !url.pathname.startsWith('/login'))
  const state = join(mkdtempSync(join(tmpdir(), 'v1per-shots-')), 'state.json')
  await context.storageState({ path: state })
  await context.close()
  return state
}

/** The IDs the shots open, read through the API with the signed-in session. */
async function ids(page: Page) {
  const get = async <T>(path: string): Promise<T> => (await page.request.get(`/api/queries/${path}`, { headers: { 'x-v1per-request': '1' } })).json() as Promise<T>
  const { nodes } = await get<NodeTree>('nodes.tree')
  const node = (name: string) => nodes.find((entry) => entry.name === name)?.id ?? ''
  const root = nodes.find((entry) => entry.depth === 0)?.id ?? ''
  const { monitors } = await get<MonitorList>(`monitors.list?node=${root}&subtree=true`)
  const checkout = monitors.find((monitor) => monitor.name === 'Checkout')
  const { incidents } = await get<IncidentPage>(`incidents.list?node=${node('Lumen Health')}`)
  return { root, lumen: node('Lumen Health'), checkout, incident: incidents[0]?.id ?? '' }
}

const section = (page: Page, heading: string) => page.getByRole('heading', { name: heading, exact: true }).locator('xpath=ancestor::section[1]')

async function view(page: Page, name: 'Orbit' | 'Wall' | 'Extended' | 'Compact') {
  await page.getByText(name, { exact: true }).click()
}

async function expand(page: Page, names: string[]) {
  for (const name of names) await page.getByRole('button', { name: `Expand ${name}`, exact: true }).click()
}

async function adminShots(browser: Browser, state: string) {
  const context = await browser.newContext({ ...desktop, storageState: state })
  const page = await context.newPage()
  await page.goto('/')
  const { root, lumen, checkout, incident } = await ids(page)

  await page.goto(`/n/${root}`)
  await view(page, 'Orbit')
  await shot(page, 'dashboard')

  await view(page, 'Wall')
  await view(page, 'Extended')
  await expand(page, ['Halden Outdoor', 'Bäckerei Bergmann', 'Lumen Health'])
  await shot(page, 'node-tree')
  await view(page, 'Compact')
  await view(page, 'Orbit')

  if (checkout) {
    await page.setViewportSize({ width: 1440, height: 1200 })
    await page.goto(`/n/${checkout.node}/monitors/${checkout.id}`)
    await page.getByRole('radio', { name: '7 d', exact: true }).click()
    await shot(page, 'monitor', section(page, 'Response time'))
  }

  await page.setViewportSize({ width: 1440, height: 1300 })
  await page.goto(`/n/${lumen}/incidents/${incident}`)
  await shot(page, 'incidents', section(page, 'Timeline'))
  await context.close()
}

/** The public page as a visitor sees it, signed out. */
async function statusPageShot(browser: Browser) {
  const context = await browser.newContext({ ...desktop, viewport: { width: 1440, height: 1400 } })
  const page = await context.newPage()
  await page.goto('/s/lumen-health')
  await shot(page, 'status-page')
  await context.close()
}

/** Records a walk from the agency to a client, its incident and its public page, as a GIF. */
async function walkthrough(browser: Browser, state: string) {
  const videos = mkdtempSync(join(tmpdir(), 'v1per-video-'))
  const size = { width: 1440, height: 900 }
  const context = await browser.newContext({ ...desktop, deviceScaleFactor: 1, storageState: state, recordVideo: { dir: videos, size } })
  const page = await context.newPage()
  const { root } = await ids(page)
  await page.goto(`/n/${root}`)
  await view(page, 'Orbit')
  await settle(page)
  await page.waitForTimeout(1500)
  const tree = page.getByLabel('Nodes and monitors')
  for (const client of ['Halden Outdoor', 'Lumen Health']) {
    await tree.getByRole('link', { name: client, exact: false }).first().click()
    await settle(page)
    await page.waitForTimeout(1800)
  }
  await page.getByText('Online appointment booking unavailable').first().click()
  await settle(page)
  await page.waitForTimeout(1800)
  await page.goto('/s/lumen-health')
  await settle(page)
  await page.waitForTimeout(1200)
  await page.mouse.wheel(0, 500)
  await page.waitForTimeout(1800)
  await context.close()

  const [video] = readdirSync(videos)
  if (!video) throw new Error('Playwright wrote no video')
  const gif = join(assets, 'walkthrough.gif')
  // The first second is the blank page before the first load.
  ffmpeg(['-ss', '1.2', '-i', join(videos, video), '-vf', 'fps=8,scale=880:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle', gif])
  rmSync(videos, { recursive: true, force: true })
  console.log('wrote walkthrough.gif')
}

function ffmpeg(args: string[]) {
  const result = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', ...args], { stdio: 'inherit' })
  if (result.status !== 0) throw new Error(`ffmpeg ${args.join(' ')} exited with ${result.status}`)
}

/** Renders `scripts/social-preview.html`, which shows a crop of `dashboard.png`, at 1280x640. */
async function socialPreview(browser: Browser) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 640 } })
  await page.goto(pathToFileURL(join(repo, 'scripts', 'social-preview.html')).href)
  await settle(page)
  await page.screenshot({ path: socialCard })
  await page.close()
  console.log('wrote .github/social-preview.png')
}

/** Re-encodes every PNG with the strongest zlib setting; lossless. */
function compress() {
  const pngs = readdirSync(assets).filter((name) => name.endsWith('.png')).map((name) => join(assets, name))
  for (const path of [...pngs, socialCard]) {
    const packed = `${path}.tmp.png`
    ffmpeg(['-i', path, '-compression_level', '100', '-pred', 'mixed', packed])
    renameSync(packed, path)
  }
}

mkdirSync(assets, { recursive: true })
const browser = await chromium.launch()
try {
  const state = await signIn(browser)
  await adminShots(browser, state)
  await statusPageShot(browser)
  await socialPreview(browser)
  await walkthrough(browser, state)
  rmSync(resolve(state, '..'), { recursive: true, force: true })
} finally {
  await browser.close()
}
compress()
