/**
 * Builds "Northwind Digital", a demo agency with three clients, for README screenshots.
 *
 * History comes through the real migration path: the script writes an Uptime Kuma 2.5.0
 * database with 90 days of rollups and 7 days of heartbeats ending now, and `v1per import kuma`
 * loads it before the first `serve`. Afterwards every monitor keeps checking live against a
 * small demo backend this script runs on `*.localhost`, a DNS responder and a probe agent, and
 * push monitors get their pushes from here. Incident updates are posted a few minutes apart,
 * so their timeline reads like one.
 *
 *   cargo build -p v1per-server && (cd apps/web && bun run build)
 *   bun scripts/demo-seed.ts [--dir /tmp/v1per-demo] [--port 8097]
 *
 * Runs until Ctrl-C, then stops everything it started. `<dir>/demo.json` holds the URL and the
 * admin sign-in for `scripts/demo-shots.ts`. A rerun replaces a directory this script created.
 */
import { Database } from 'bun:sqlite'
import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseArgs } from 'node:util'

import type { Commands } from '../apps/web/src/generated/commands'
import type { Queries } from '../apps/web/src/generated/queries'
import type { Impact, IncidentState, Locale, MonitorId, NodeId, PageGroupInput } from '../apps/web/src/generated'

const repo = resolve(import.meta.dir, '..')
const { values: options } = parseArgs({
  options: { dir: { type: 'string', default: '/tmp/v1per-demo' }, port: { type: 'string', default: '8097' } },
})
const dir = resolve(options.dir)
const port = Number(options.port)
const backendPort = port + 1
const dnsPort = port + 2
const origin = `http://127.0.0.1:${port}`
const binary = process.env.V1PER_BIN ?? join(repo, 'target', 'debug', 'v1per')
const webDir = join(repo, 'apps', 'web', 'dist')
const admin = { email: 'mara.lindqvist@northwind.example', password: crypto.randomUUID() }

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
/** Days of rollups, and of raw heartbeats, the Kuma database holds. */
const HISTORY_DAYS = 90
const RAW_DAYS = 7
/** Time between the incident updates the script posts. */
const UPDATE_GAP = 3 * MINUTE

// The demo agency

type Check =
  | { type: 'http' }
  | { type: 'keyword'; keyword: string }
  | { type: 'json-query'; path: string; expected: string }
  | { type: 'port' }
  | { type: 'dns' }
  | { type: 'push' }

type DemoMonitor = Check & {
  name: string
  interval: number
  /** Typical response time in ms; the backend answers this slowly too. */
  latency: number
  /** Past outages as `[days ago, minutes]`. */
  outages?: Array<[number, number]>
  /** Failing since this many minutes before the seed, and still failing. */
  failingFor?: number
}

interface DemoPage {
  slug: string
  title: string
  description: string
  locale: Locale
}

interface DemoNode {
  name: string
  slug: string
  monitors: DemoMonitor[]
  children: DemoNode[]
  page?: DemoPage
}

const agency: DemoNode = {
  name: 'Northwind Digital',
  slug: 'northwind',
  monitors: [
    { name: 'Agency website', type: 'keyword', keyword: 'Northwind', interval: 60, latency: 95 },
    { name: 'Client portal', type: 'http', interval: 60, latency: 140, outages: [[70, 15]] },
  ],
  children: [
    {
      name: 'Halden Outdoor',
      slug: 'halden',
      monitors: [],
      page: {
        slug: 'halden-outdoor',
        title: 'Halden Outdoor status',
        description: 'Live status of the Halden Outdoor shop, checkout and warehouse services.',
        locale: 'en',
      },
      children: [
        {
          name: 'Web shop',
          slug: 'halden',
          children: [],
          monitors: [
            { name: 'Storefront', type: 'http', interval: 60, latency: 180, outages: [[12.3, 18]] },
            { name: 'Checkout', type: 'keyword', keyword: 'Place order', interval: 60, latency: 240, outages: [[3.4, 25], [41, 12]] },
            { name: 'Product search API', type: 'json-query', path: 'status', expected: 'ok', interval: 60, latency: 85, outages: [[27, 50]] },
            { name: 'Shop DNS', type: 'dns', interval: 300, latency: 4 },
          ],
        },
        {
          name: 'Warehouse',
          slug: 'halden',
          children: [],
          monitors: [
            { name: 'Inventory sync', type: 'push', interval: 120, latency: 1450, outages: [[1.6, 65]] },
            { name: 'ERP gateway', type: 'port', interval: 60, latency: 2, outages: [[22, 40]] },
            { name: 'Shipping labels', type: 'http', interval: 120, latency: 310, outages: [[6.2, 35]] },
          ],
        },
      ],
    },
    {
      name: 'Bäckerei Bergmann',
      slug: 'bergmann',
      monitors: [],
      page: {
        slug: 'baeckerei-bergmann',
        title: 'Bäckerei Bergmann Systemstatus',
        description: 'Aktueller Status von Online-Shop, Bestellungen und Filialsystemen.',
        locale: 'de',
      },
      children: [
        {
          name: 'Bestellsystem',
          slug: 'bergmann',
          children: [],
          monitors: [
            { name: 'Online-Shop', type: 'keyword', keyword: 'Brot', interval: 60, latency: 160, outages: [[9, 14]] },
            { name: 'Bestell-API', type: 'json-query', path: 'status', expected: 'ok', interval: 60, latency: 70, outages: [[9, 22]] },
            { name: 'Zahlungs-Webhook', type: 'push', interval: 60, latency: 45 },
          ],
        },
        {
          name: 'Filialen',
          slug: 'bergmann',
          children: [],
          monitors: [
            { name: 'Kassen-Sync', type: 'push', interval: 300, latency: 900, outages: [[0.45, 30]] },
            { name: 'Filial-VPN', type: 'port', interval: 60, latency: 3, outages: [[19, 120], [52, 45]] },
            { name: 'Filial-DNS', type: 'dns', interval: 300, latency: 5 },
          ],
        },
      ],
    },
    {
      name: 'Lumen Health',
      slug: 'lumen',
      monitors: [],
      page: {
        slug: 'lumen-health',
        title: 'Lumen Health service status',
        description: 'Status of the patient portal, online booking and video consultations.',
        locale: 'en',
      },
      children: [
        {
          name: 'Patient portal',
          slug: 'lumen',
          children: [],
          monitors: [
            { name: 'Patient portal', type: 'keyword', keyword: 'Sign in', interval: 60, latency: 210, outages: [[33, 20]] },
            { name: 'Login service', type: 'http', interval: 60, latency: 120, outages: [[33, 20]] },
            { name: 'Video consultations', type: 'http', interval: 60, latency: 820 },
          ],
        },
        {
          name: 'Booking',
          slug: 'lumen',
          children: [],
          monitors: [
            { name: 'Appointment booking API', type: 'json-query', path: 'status', expected: 'ok', interval: 60, latency: 130, outages: [[15, 30]], failingFor: 42 },
            { name: 'SMS reminders', type: 'push', interval: 300, latency: 600 },
            { name: 'Calendar sync', type: 'http', interval: 120, latency: 260, outages: [[15, 30]] },
            { name: 'Database replica', type: 'port', interval: 60, latency: 2, outages: [[15, 30]] },
          ],
        },
      ],
    },
  ],
}

const slug = (text: string) =>
  text
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')

interface Placed {
  monitor: DemoMonitor
  node: DemoNode
  host: string
}

/** Every monitor with its node and the backend hostname it checks. */
function placed(node: DemoNode): Placed[] {
  const own = node.monitors.map((monitor) => ({ monitor, node, host: `${slug(monitor.name)}.${node.slug}.localhost` }))
  return [...own, ...node.children.flatMap(placed)]
}

const monitors = placed(agency)
const byHost = new Map(monitors.map((entry) => [entry.host, entry]))

/** mulberry32: the same agency, outages and latencies on every run. */
function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** A response time around `typical`: busier in the afternoon, with noise and rare spikes. */
function latencyAt(typical: number, at: number, next: () => number): number {
  const hour = (at / HOUR + 2) % 24
  const daily = 1 + 0.18 * Math.sin(((hour - 9) / 24) * 2 * Math.PI)
  const spike = next() < 0.0015 ? 1.6 + next() : 1
  return Math.max(1, Math.round(typical * daily * (0.82 + next() * 0.36) * spike))
}

const pushTokens = new Map(monitors.map(({ monitor }, index) => [monitor.name, `nw${index}${slug(monitor.name)}`.replace(/-/g, '').padEnd(24, 'x')]))

// The Kuma 2.5.0 database

function upMessage(check: Check, latency: number): string {
  switch (check.type) {
    case 'http':
      return '200 - OK'
    case 'keyword':
      return '200 - OK, keyword is found'
    case 'json-query':
      return `JSON query passes (comparing ${check.expected} == ${check.expected})`
    case 'port':
      return `${latency} ms`
    case 'dns':
      return 'Records: 203.0.113.10'
    case 'push':
      return 'OK'
  }
}

function downMessage(check: Check): string {
  switch (check.type) {
    case 'port':
      return 'Connection failed'
    case 'dns':
      return 'queryA ETIMEOUT'
    case 'push':
      return 'No heartbeat in the time window'
    default:
      return 'Request failed with status code 503'
  }
}

interface Bucket {
  up: number
  down: number
  ping: number
  min: number
  max: number
}

/** Kuma's `uptime-calculator.js` bucket update: a running average over up beats only. */
function count(buckets: Map<number, Bucket>, key: number, ping: number | undefined) {
  const bucket = buckets.get(key) ?? { up: 0, down: 0, ping: 0, min: 0, max: 0 }
  buckets.set(key, bucket)
  if (ping === undefined) {
    bucket.down += 1
    return
  }
  bucket.up += 1
  if (bucket.up === 1) Object.assign(bucket, { ping, min: ping, max: ping })
  else {
    bucket.ping = (bucket.ping * (bucket.up - 1) + ping) / bucket.up
    bucket.min = Math.min(bucket.min, ping)
    bucket.max = Math.max(bucket.max, ping)
  }
}

const kumaTime = (ms: number) => new Date(ms).toISOString().replace('T', ' ').replace('Z', '')

/**
 * Writes the agency as a Kuma 2.5.0 database: nodes as group monitors, then every monitor with
 * heartbeats from a simulated check loop and the `stat_*` rollups Kuma keeps of them.
 */
function writeKumaDatabase(path: string, now: number) {
  const schema = readFileSync(join(repo, 'fixtures', 'kuma-2.5.0', 'schema.sql'), 'utf8')
  const db = new Database(path)
  db.run('PRAGMA journal_mode = DELETE')
  const statements = schema
    .split('\n')
    .filter((line) => !line.startsWith('--'))
    .join('\n')
    .split(/;\s*\n/)
    .map((statement) => statement.trim())
    .filter((statement) => statement && !statement.startsWith('CREATE TABLE sqlite_'))
  const migrations = schema.split('\n').flatMap((line) => (line.startsWith('-- migration: ') ? [line.slice(14).trim()] : []))
  const created = kumaTime(now - HISTORY_DAYS * DAY).slice(0, 19)

  db.transaction(() => {
    for (const statement of statements) db.run(statement)
    migrations.forEach((name, index) => db.run('INSERT INTO knex_migrations (name, batch, migration_time) VALUES (?, 1, ?)', [name, now - HISTORY_DAYS * DAY + index]))

    const insertMonitor = db.prepare(
      `INSERT INTO monitor (name, type, parent, interval, url, hostname, port, keyword, json_path, json_path_operator,
         expected_value, dns_resolve_type, dns_resolve_server, push_token, timeout, created_date)
       VALUES ($name, $type, $parent, $interval, $url, $hostname, $port, $keyword, $json_path, $operator,
         $expected, $dns_type, $dns_server, $push_token, $timeout, $created) RETURNING id`
    )
    const beats: Array<[monitor: number, time: number, ping: number | undefined, message: string, important: boolean]> = []
    const statRows: Array<[table: string, monitor: number, key: number, bucket: Bucket]> = []

    const addNode = (node: DemoNode, parent: number | null) => {
      for (const { monitor, host } of placed({ ...node, children: [] })) {
        const http = monitor.type === 'http' || monitor.type === 'keyword' || monitor.type === 'json-query'
        const row = insertMonitor.get({
          $name: monitor.name,
          $type: monitor.type,
          $parent: parent,
          $interval: monitor.interval,
          $url: http ? `http://${host}:${backendPort}/${monitor.type === 'json-query' ? 'health' : ''}` : null,
          $hostname: monitor.type === 'port' || monitor.type === 'dns' ? host : null,
          $port: monitor.type === 'port' ? backendPort : monitor.type === 'dns' ? dnsPort : null,
          $keyword: monitor.type === 'keyword' ? monitor.keyword : null,
          $json_path: monitor.type === 'json-query' ? monitor.path : null,
          $operator: monitor.type === 'json-query' ? '==' : null,
          $expected: monitor.type === 'json-query' ? monitor.expected : null,
          $dns_type: monitor.type === 'dns' ? 'A' : null,
          $dns_server: monitor.type === 'dns' ? '127.0.0.1' : null,
          $push_token: monitor.type === 'push' ? (pushTokens.get(monitor.name) ?? null) : null,
          $timeout: Math.min(monitor.interval * 0.8, 30),
          $created: created,
        }) as { id: number }
        simulate(row.id, monitor)
      }
      for (const child of node.children) {
        const group = insertMonitor.get({
          $name: child.name, $type: 'group', $parent: parent, $interval: 60, $url: null, $hostname: null, $port: null,
          $keyword: null, $json_path: null, $operator: null, $expected: null, $dns_type: null, $dns_server: null,
          $push_token: null, $timeout: 48, $created: created,
        }) as { id: number }
        addNode(child, group.id)
      }
    }

    const simulate = (id: number, monitor: DemoMonitor) => {
      const next = random(id * 7919)
      const failingSince = monitor.failingFor === undefined ? Infinity : now - monitor.failingFor * MINUTE
      const windows = (monitor.outages ?? []).map(([daysAgo, minutes]) => [now - daysAgo * DAY, now - daysAgo * DAY + minutes * MINUTE] as const)
      // Blips of a minute or three on about one day in forty.
      for (let day = 1; day < HISTORY_DAYS; day++) {
        if (monitor.type !== 'push' && next() < 0.025) {
          const start = now - day * DAY + next() * DAY
          windows.push([start, start + (1 + Math.floor(next() * 3)) * MINUTE])
        }
      }
      const minutes = new Map<number, Bucket>()
      const hours = new Map<number, Bucket>()
      const days = new Map<number, Bucket>()
      let previous: boolean | undefined
      const step = monitor.interval * 1000
      const offset = next() * step
      for (let at = now - HISTORY_DAYS * DAY + offset; at <= now; at += step) {
        const time = Math.round(at + next() * 400)
        const down = time >= failingSince || windows.some(([from, to]) => time >= from && time < to)
        const ping = down ? undefined : latencyAt(monitor.latency, time, next)
        const seconds = Math.floor(time / 1000)
        count(minutes, seconds - (seconds % 60), ping)
        count(hours, seconds - (seconds % 3600), ping)
        count(days, seconds - (seconds % 86_400), ping)
        if (time >= now - RAW_DAYS * DAY) {
          beats.push([id, time, ping, down ? downMessage(monitor) : upMessage(monitor, ping ?? 0), previous !== down])
        }
        previous = down
      }
      // Kuma keeps minute rows for a day and hour rows for 30 days.
      const last = Math.floor(now / 1000)
      for (const [key, bucket] of minutes) if (key >= last - 86_400) statRows.push(['stat_minutely', id, key, bucket])
      for (const [key, bucket] of hours) if (key >= last - 30 * 86_400) statRows.push(['stat_hourly', id, key, bucket])
      for (const [key, bucket] of days) statRows.push(['stat_daily', id, key, bucket])
    }

    addNode(agency, null)

    const insertStats = (table: string) =>
      db.prepare(`INSERT INTO ${table} (monitor_id, timestamp, ping, ping_min, ping_max, up, down) VALUES (?, ?, ?, ?, ?, ?, ?)`)
    const stats = new Map(['stat_minutely', 'stat_hourly', 'stat_daily'].map((table) => [table, insertStats(table)]))
    for (const [table, monitor, key, bucket] of statRows) {
      stats.get(table)?.run(monitor, key, bucket.ping, bucket.min, bucket.max, bucket.up, bucket.down)
    }
    // Kuma numbers heartbeats in time order across monitors.
    beats.sort((a, b) => a[1] - b[1])
    const insertBeat = db.prepare('INSERT INTO heartbeat (important, monitor_id, status, msg, time, ping) VALUES (?, ?, ?, ?, ?, ?)')
    for (const [monitor, time, ping, message, important] of beats) {
      insertBeat.run(important ? 1 : 0, monitor, ping === undefined ? 0 : 1, message, kumaTime(time), ping ?? null)
    }
  })()
  db.close()
}

// The demo backend: what the monitors check

/** Answers every monitored hostname like the service it stands for, as slowly as it usually is. */
function startBackend() {
  return Bun.serve({
    hostname: '127.0.0.1',
    port: backendPort,
    async fetch(request) {
      const host = new URL(request.url).hostname
      const entry = byHost.get(host)
      if (!entry) return Response.json({ ok: true })
      const { monitor } = entry
      await Bun.sleep(latencyAt(monitor.latency, Date.now(), Math.random) * 0.9)
      if (monitor.failingFor !== undefined) return Response.json({ status: 'unavailable' }, { status: 503 })
      if (monitor.type === 'json-query') return Response.json({ status: 'ok', version: '4.12.0' })
      const keyword = monitor.type === 'keyword' ? monitor.keyword : ''
      return new Response(`<!doctype html><title>${entry.node.name}</title><p>${keyword}</p>`, {
        headers: { 'content-type': 'text/html; charset=utf-8' },
      })
    },
  })
}

/** A DNS responder for the dns monitors: one A record, 203.0.113.10, for every name. */
async function startDns() {
  return Bun.udpSocket({
    hostname: '127.0.0.1',
    port: dnsPort,
    socket: {
      data(socket, query, fromPort, address) {
        let end = 12
        while (query[end] !== 0 && end < query.length) end += (query[end] ?? 0) + 1
        const question = query.subarray(12, end + 5)
        const isA = query[end + 1] === 0 && query[end + 2] === 1
        const header = Buffer.from([query[0] ?? 0, query[1] ?? 0, 0x81, 0x80, 0, 1, 0, isA ? 1 : 0, 0, 0, 0, 0])
        const answer = Buffer.from([0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 1, 0x2c, 0, 4, 203, 0, 113, 10])
        socket.send(Buffer.concat([header, question, isA ? answer : Buffer.alloc(0)]), fromPort, address)
      },
    },
  })
}

// The API

class Api {
  private cookies = new Map<string, string>()

  async command<N extends keyof Commands>(name: N, args: Commands[N]['args']): Promise<Commands[N]['result']> {
    return this.send(`/api/commands/${name}`, {
      method: 'POST',
      body: JSON.stringify({ idempotency_key: crypto.randomUUID(), ...args }),
    })
  }

  async query<N extends keyof Queries>(name: N, args: Queries[N]['args']): Promise<Queries[N]['result']> {
    const params = new URLSearchParams()
    for (const [key, value] of Object.entries(args)) if (value !== undefined) params.append(key, String(value))
    return this.send(`/api/queries/${name}?${params}`, { method: 'GET' })
  }

  private async send<T>(path: string, init: RequestInit): Promise<T> {
    const cookie = [...this.cookies].map(([name, value]) => `${name}=${value}`).join('; ')
    const response = await fetch(`${origin}${path}`, {
      ...init,
      headers: { 'content-type': 'application/json', 'x-v1per-request': '1', origin, cookie },
    })
    for (const line of response.headers.getSetCookie()) {
      const [pair = ''] = line.split(';')
      const at = pair.indexOf('=')
      this.cookies.set(pair.slice(0, at), pair.slice(at + 1))
    }
    if (!response.ok) throw new Error(`${path}: ${response.status} ${await response.text()}`)
    return (await response.json()) as T
  }
}

// Processes

const children: ChildProcess[] = []

function start(args: string[]): ChildProcess {
  const child = spawn(binary, args, { stdio: ['ignore', 'ignore', 'inherit'], env: { ...process.env, V1PER_LOG: 'warn' } })
  if (child.pid === undefined) throw new Error(`v1per ${args[0]} did not start`)
  console.log(`started v1per ${args[0]}, pid ${child.pid}`)
  children.push(child)
  return child
}

function run(args: string[]) {
  const result = spawnSync(binary, args, { stdio: ['ignore', 'ignore', 'inherit'] })
  if (result.status !== 0) throw new Error(`v1per ${args.slice(0, 2).join(' ')} exited with ${result.status}`)
}

async function waitFor(what: string, ready: () => Promise<boolean>) {
  const deadline = Date.now() + 60_000
  while (Date.now() < deadline) {
    if (await ready().catch(() => false)) return
    await Bun.sleep(250)
  }
  throw new Error(`${what} was not ready within 60 s`)
}

// Seeding

async function seed(api: Api, now: number) {
  await api.command('auth.login', admin)
  const { nodes } = await api.query('nodes.tree', {})
  const nodeId = new Map(nodes.flatMap((node) => (node.name ? [[node.name, node.id] as const] : [])))
  const root = nodes.find((node) => node.depth === 0)?.id
  if (!root) throw new Error('no root node')
  await api.command('nodes.rename', { node: root, name: agency.name })
  nodeId.set(agency.name, root)
  const node = (name: string): NodeId => {
    const id = nodeId.get(name)
    if (!id) throw new Error(`no node ${name}`)
    return id
  }

  // The import brings Kuma's groups as nodes and as group monitors; the nodes are enough.
  const { monitors: all } = await api.query('monitors.list', { node: root, subtree: true })
  for (const group of all.filter((entry) => entry.type === 'group')) await api.command('monitors.delete', { monitor: group.id })
  const monitorId = new Map(all.filter((entry) => entry.type !== 'group').map((entry) => [entry.name, entry.id]))
  const monitor = (name: string): MonitorId => {
    const id = monitorId.get(name)
    if (!id) throw new Error(`no monitor ${name}`)
    return id
  }

  await api.command('channels.create', {
    node: root,
    name: 'Slack #ops-alerts',
    apply_existing: true,
    is_default: true,
    settings: { type: 'slack', webhook_url: { value: `http://hooks.northwind.localhost:${backendPort}/slack`, set: true }, channel: '#ops-alerts' },
  })
  await api.command('channels.create', {
    node: node('Lumen Health'),
    name: 'Lumen on-call webhook',
    apply_existing: true,
    settings: { type: 'webhook', url: { value: `http://oncall.lumen.localhost:${backendPort}/hook`, set: true }, content_type: 'json', additional_headers: { set: false } },
  })

  await addProbe(api, root, ['Storefront', 'Checkout', 'Patient portal', 'Appointment booking API', 'Online-Shop'].map(monitor))

  for (const client of agency.children) {
    if (client.page) await addPage(api, node(client.name), client, monitor)
  }

  const tomorrow = new Date(now + DAY).toISOString().slice(0, 10)
  await api.command('maintenance.create', {
    node: node('Bäckerei Bergmann'),
    title: 'Update der Kassensysteme',
    description: 'Die Kassen in allen Filialen erhalten ein Software-Update. Kartenzahlung ist in dieser Zeit nicht möglich.',
    schedule: { strategy: 'single', start_date: `${tomorrow}T21:00`, end_date: `${tomorrow}T23:00`, timezone: 'Europe/Vienna' },
    monitors: [monitor('Kassen-Sync'), monitor('Filial-VPN')],
  })

  await addIncident(api, node('Lumen Health'), 'lumen-health', {
    title: 'Online appointment booking unavailable',
    impacts: [
      { target: { type: 'monitor', monitor: monitor('Appointment booking API') }, impact: 'major_outage' },
      { target: { type: 'monitor', monitor: monitor('Patient portal') }, impact: 'degraded' },
    ],
    updates: [
      ['investigating', 'Patients cannot book new appointments online. Existing appointments are not affected. We are looking into it.'],
      ['identified', 'The booking API lost its connection to the scheduling database after a failed failover. We are restoring the primary database.'],
      ['identified', 'The restore is about two thirds done. Booking by phone at the front desk works as usual.'],
    ],
  })
  await addIncident(api, node('Halden Outdoor'), 'halden-outdoor', {
    title: 'Slow checkout for some customers',
    impacts: [{ target: { type: 'monitor', monitor: monitor('Checkout') }, impact: 'degraded' }],
    updates: [
      ['investigating', 'Some customers see the payment step take up to 30 seconds.'],
      ['monitoring', 'Our payment provider rolled back a faulty release. Checkout times are back to normal and we are watching them.'],
      ['resolved', 'Checkout has been fast for 30 minutes. Sorry for the wait.'],
    ],
  })
}

async function addProbe(api: Api, root: NodeId, assigned: MonitorId[]) {
  const created = await api.command('probes.create', { node: root, name: 'Frankfurt' })
  if (!created.secret) throw new Error('the probe token was withheld')
  start(['probe', '--server', origin, '--token', created.secret, '--data-dir', join(dir, 'probe')])
  await waitFor('the Frankfurt probe', async () => {
    const { probes } = await api.query('probes.list', { node: root })
    return probes.some((probe) => probe.id === created.probe.id && probe.status === 'online')
  })
  const { probes } = await api.query('probes.list', { node: root })
  const builtin = probes.find((probe) => probe.builtin)
  if (!builtin) throw new Error('no built-in probe')
  for (const id of assigned) {
    const { monitor: m } = await api.query('monitors.get', { monitor: id })
    await api.command('monitors.update', {
      monitor: id,
      expected_revision: m.revision,
      name: m.name,
      description: m.description,
      settings: m.settings,
      interval: m.interval,
      retry_interval: m.retry_interval,
      max_retries: m.max_retries,
      resend_interval: m.resend_interval,
      timeout: m.timeout,
      upside_down: m.upside_down,
      quorum: m.quorum,
      tags: m.tags,
      channels: m.channels,
      probes: [builtin.id, created.probe.id],
    })
  }
}

async function addPage(api: Api, node: NodeId, client: DemoNode, monitor: (name: string) => MonitorId) {
  const page = client.page
  if (!page) return
  const { page: created } = await api.command('pages.create', {
    node,
    slug: page.slug,
    title: page.title,
    description: page.description,
    footer: page.locale === 'de' ? 'Fragen? info@baeckerei-bergmann.example' : `Questions? Write to support@${client.slug}.example`,
    theme: 'auto',
    timezone: 'Europe/Vienna',
    default_locale: page.locale,
  })
  const groups: PageGroupInput[] = client.children.map((team) => ({
    name: team.name,
    components: team.monitors.map((entry) => ({ target: { type: 'monitor', monitor: monitor(entry.name) } })),
  }))
  await api.command('pages.updateLayout', { page: created.id, expected_revision: created.revision, groups })
  await api.command('pages.updateAccess', { page: created.id, public: true })
}

interface DemoIncident {
  title: string
  impacts: Array<{ target: { type: 'monitor'; monitor: MonitorId }; impact: Impact }>
  updates: Array<[IncidentState, string]>
}

/** Opens the incident, publishes it, then posts its updates `UPDATE_GAP` apart in the background. */
async function addIncident(api: Api, node: NodeId, pageSlug: string, incident: DemoIncident) {
  const [opening, ...updates] = incident.updates
  if (!opening) throw new Error(`${incident.title} has no updates`)
  const [state, message] = opening
  const { incident: created } = await api.command('incidents.create', { node, title: incident.title, state, message, impacts: incident.impacts })
  const { pages } = await api.query('pages.list', { node })
  const page = pages.find((entry) => entry.slug === pageSlug)
  if (page) await api.command('incidents.publish', { incident: created.id, page: page.id })
  updates.forEach(([state, message], index) => {
    setTimeout(() => {
      api
        .command('incidents.addUpdate', { incident: created.id, state, message })
        .then(() => console.log(`posted "${state}" on "${incident.title}"`))
        .catch((error: unknown) => console.error(error))
    }, (index + 1) * UPDATE_GAP)
  })
}

/** Pushes for every push monitor once a minute, so they stay up. */
function startPushes() {
  const push = () => {
    for (const { monitor } of monitors.filter((entry) => entry.monitor.type === 'push')) {
      const ping = latencyAt(monitor.latency, Date.now(), Math.random)
      fetch(`${origin}/api/push/${pushTokens.get(monitor.name)}?status=up&msg=OK&ping=${ping}`).catch(() => {})
    }
  }
  push()
  return setInterval(push, MINUTE)
}

// Main

if (!existsSync(join(webDir, 'index.html'))) throw new Error(`${webDir} is missing: run \`bun run build\` in apps/web`)
if (!existsSync(binary)) throw new Error(`${binary} is missing: run \`cargo build -p v1per-server\``)
if (existsSync(dir)) {
  if (!existsSync(join(dir, 'demo.json'))) throw new Error(`${dir} exists and was not made by this script`)
  rmSync(dir, { recursive: true, force: true })
}
mkdirSync(dir, { recursive: true })
writeFileSync(join(dir, 'demo.json'), '{}')

const data = join(dir, 'data')
const passwordFile = join(dir, 'password')
writeFileSync(passwordFile, admin.password, { mode: 0o600 })
run(['init', '--data-dir', data, '--admin-email', admin.email, '--admin-password-file', passwordFile])

const now = Date.now()
const kuma = join(dir, 'kuma.db')
writeKumaDatabase(kuma, now)
console.log(`wrote ${kuma}, importing`)
run(['import', 'kuma', '--data-dir', data, '--source', kuma, '--into', 'Root'])

const backend = startBackend()
const dns = await startDns()
const server = start(['serve', '--data-dir', data, '--bind', `127.0.0.1:${port}`, '--web-dir', webDir, '--instance-name', agency.name])
await waitFor('v1per serve', async () => (await fetch(`${origin}/health`)).ok)
const pushes = startPushes()

/** Stops the agent, then the server, waiting for each to exit, then the backend. */
async function stop(code = 0) {
  server.removeAllListeners('exit')
  clearInterval(pushes)
  for (const child of children.reverse()) {
    if (child.exitCode !== null || child.pid === undefined) continue
    const exited = new Promise((done) => child.once('exit', done))
    process.kill(child.pid, 'SIGTERM')
    await exited
    console.log(`stopped v1per ${child.spawnargs[1]}, pid ${child.pid}`)
  }
  backend.stop(true)
  dns.close()
  process.exit(code)
}
process.on('SIGINT', () => stop())
process.on('SIGTERM', () => stop())
server.once('exit', (code) => {
  console.error(`v1per serve exited with ${code}`)
  stop(1)
})

try {
  await seed(new Api(), now)
} catch (error) {
  console.error(error)
  await stop(1)
}
writeFileSync(join(dir, 'demo.json'), JSON.stringify({ url: origin, ...admin }, null, 2))
console.log(`\nNorthwind Digital runs on ${origin}, sign in as ${admin.email} / ${admin.password}`)
console.log(`incident updates follow over the next ${(UPDATE_GAP * 2) / MINUTE} minutes; Ctrl-C stops everything`)
