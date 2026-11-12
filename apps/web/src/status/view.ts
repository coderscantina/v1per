import type {
  ComponentDetail,
  Impact,
  IncidentState,
  PageAudience,
  PageBar,
  PageHour,
  PageSnapshot,
  PageStatus,
  Postmortem,
  PublicIncident,
  PublicIncidentUpdate,
  ResponseRange,
} from '~/generated'

import { rangeDays, rangeSpan } from './range'
import { slaView, type SlaView } from './sla'
import { componentBasis, countsOf, pageBasis, uptimeOf, type Counts, type UptimeBasis } from './uptime'

/** A status as the page paints it: one of the page statuses, or `none` for a day or monitor without data. */
export type Tone = PageStatus | 'none'

/** When a bar starts: a local day of the page (`YYYY-MM-DD`), or a UTC hour (Unix ms). */
export type BarTime = { day: string } | { hour: number }

export interface BarView {
  time: BarTime
  status: Tone
  uptime?: number
  approximate: boolean
  maintenance: boolean
  incidents: string[]
  /** The checks behind `uptime`. */
  counts: Counts
}

export interface ComponentView {
  id: string
  name: string
  description: string
  status: Tone
  /** Local days, oldest first, as far back as the audience's longest range. */
  days: BarView[]
  /** The last 24 hours, oldest first. */
  hours: BarView[]
  detail?: ComponentDetail
}

export interface GroupView {
  id: string
  name: string
  defaultOpen: boolean
  /** The worst status among the group's components, for a collapsed group. */
  status: Tone
  components: ComponentView[]
}

export interface AffectedView {
  name: string
  impact?: Impact
}

/** One update on an incident's timeline. */
export interface TimelineEntry {
  state: IncidentState
  message: string
  at: number
  /** The named components whose impact this update changed. */
  changes: AffectedView[]
}

export interface IncidentView {
  id: string
  title: string
  state: IncidentState
  impact?: Impact
  startedAt: number
  resolvedAt?: number
  /** Newest first. */
  timeline: TimelineEntry[]
  affected: AffectedView[]
  /** The worst impact any update set: what a resolved incident was at its worst. */
  peak?: Impact
  postmortem?: Postmortem
}

/** Items of one calendar month in the visitor's timezone; `at` is the first item's start. */
export interface MonthGroup<T> {
  key: string
  at: number
  items: T[]
}

export interface MaintenanceView {
  title: string
  description: string
  active: boolean
  startsAt?: number
  endsAt?: number
  components: string[]
}

/** The page's headline figures over its time frame. */
export interface PageFigures {
  /** The mean of the components' uptime; absent when none has data. */
  uptime?: number
  /** What `uptime` stands on. */
  basis: UptimeBasis
  /** Incidents open now or resolved within the time frame. */
  incidents: number
  /** When the next maintenance starts: `now` while one is under way, absent when none is planned. */
  maintenance?: number | 'now'
}

export interface PageView {
  title: string
  description: string
  footer: string
  logo?: string
  status: PageStatus
  audience: PageAudience
  groups: GroupView[]
  /** Open incidents, newest first. The resolved ones come from `incidents.json`. */
  active: IncidentView[]
  /** Component names by ID, to build views of incidents that arrive later. */
  names: ReadonlyMap<string, string>
  maintenance: MaintenanceView[]
  /** The SLAs the page shows, in the page's order. */
  slas: SlaView[]
  /** When each listed incident started and ended, for counting them per time frame. */
  incidentSpans: { startedAt: number; resolvedAt?: number }[]
  /** When the next maintenance starts: `now` while one is under way, absent when none is planned. */
  nextMaintenance?: number | 'now'
  /** The longest time frame the audience may pick. */
  rangeMax: ResponseRange
  generatedAt: number
}

/** A component's bars over `range`: hours over 24 hours, days otherwise. */
export function barsIn(component: ComponentView, range: ResponseRange): BarView[] {
  return range === '24h' ? component.hours : component.days.slice(-rangeDays[range])
}

/** The headline figures over `range`, at `now`. */
export function pageFigures(view: PageView, range: ResponseRange, now: number): PageFigures {
  const bases = view.groups.flatMap((group) => group.components.map((component) => componentBasis(barsIn(component, range))))
  const uptimes = bases.flatMap((basis) => uptimeOf(basis.counts) ?? [])
  const since = now - rangeSpan(range)
  return {
    uptime: uptimes.length > 0 ? uptimes.reduce((sum, uptime) => sum + uptime, 0) / uptimes.length : undefined,
    basis: pageBasis(bases),
    incidents: view.incidentSpans.filter((span) => (span.resolvedAt ?? now) >= since).length,
    maintenance: view.nextMaintenance,
  }
}

const rank: Record<Tone, number> = {
  none: 0,
  operational: 1,
  maintenance: 2,
  degraded: 3,
  partial_outage: 4,
  major_outage: 5,
}

/** The worse of statuses; `none` when there are none. */
export function worst(statuses: Iterable<Tone>): Tone {
  let found: Tone = 'none'
  for (const status of statuses) if (rank[status] > rank[found]) found = status
  return found
}

/** Each named component's impact from the newest update that set one, else the incident's own. */
function affected(incident: PublicIncident, names: ReadonlyMap<string, string>): AffectedView[] {
  const impacts = new Map<string, Impact>()
  for (const update of incident.updates) {
    for (const { component, impact } of update.impacts) if (!impacts.has(component)) impacts.set(component, impact)
  }
  return incident.components.flatMap((id) => {
    const name = names.get(id)
    return name === undefined ? [] : [{ name, impact: impacts.get(id) ?? incident.impact }]
  })
}

/**
 * The updates as timeline entries, newest first. An update's changes are the impacts it set that
 * differ from what the older updates left, so a repeated impact is not listed again.
 */
function timeline(updates: PublicIncidentUpdate[], names: ReadonlyMap<string, string>): TimelineEntry[] {
  const current = new Map<string, Impact>()
  const entries = [...updates].reverse().map((update): TimelineEntry => {
    const changes = update.impacts.flatMap(({ component, impact }) => {
      const previous = current.get(component)
      current.set(component, impact)
      const name = names.get(component)
      return previous === impact || name === undefined ? [] : [{ name, impact }]
    })
    return { state: update.state, message: update.message, at: update.created_at, changes }
  })
  return entries.reverse()
}

/** Groups items by the month they started in, keeping their order. */
export function byMonth<T extends { startedAt: number }>(items: readonly T[]): MonthGroup<T>[] {
  const groups = new Map<string, MonthGroup<T>>()
  for (const item of items) {
    const date = new Date(item.startedAt)
    const key = `${date.getFullYear()}-${date.getMonth() + 1}`
    const group = groups.get(key)
    if (group) group.items.push(item)
    else groups.set(key, { key, at: item.startedAt, items: [item] })
  }
  return [...groups.values()]
}

/** An incident as the page draws it, with its components named from `names`. */
export function incidentView(incident: PublicIncident, names: ReadonlyMap<string, string>): IncidentView {
  return {
    id: incident.id,
    title: incident.title,
    state: incident.state,
    impact: incident.impact,
    startedAt: incident.started_at,
    resolvedAt: incident.resolved_at,
    timeline: timeline(incident.updates, names),
    affected: affected(incident, names),
    peak: peakImpact(incident),
    postmortem: incident.postmortem,
  }
}

const impactRank: Record<Impact, number> = { operational: 0, degraded: 1, partial_outage: 2, major_outage: 3 }

/** The worst impact any update set, above operational; absent when none did. */
function peakImpact(incident: PublicIncident): Impact | undefined {
  let peak: Impact | undefined
  for (const update of incident.updates) {
    for (const { impact } of update.impacts) if (impact !== 'operational' && (!peak || impactRank[impact] > impactRank[peak])) peak = impact
  }
  return peak
}

/**
 * Turns a snapshot into what the page draws. The snapshot names components and incidents by
 * ID only; this resolves them to names and titles and keeps the open incidents.
 */
export function pageView(snapshot: PageSnapshot, base: string): PageView {
  const names = new Map<string, string>()
  for (const group of snapshot.groups) for (const component of group.components) names.set(component.id, component.name)
  const titles = new Map(snapshot.incidents.map((incident) => [incident.id, incident.title]))

  const bar = (wire: PageBar | PageHour, time: BarTime): BarView => ({
    time,
    status: wire.status ?? 'none',
    uptime: wire.uptime,
    approximate: 'utc_approximate' in wire ? (wire.utc_approximate ?? false) : false,
    maintenance: wire.maintenance ?? false,
    incidents: (wire.incidents ?? []).flatMap((id) => titles.get(id) ?? []),
    counts: countsOf(wire),
  })

  const groups = snapshot.groups.map((group): GroupView => {
    const components = group.components.map(
      (component): ComponentView => ({
        id: component.id,
        name: component.name,
        description: component.description,
        status: component.status ?? 'none',
        detail: component.detail,
        days: component.bars.map((day) => bar(day, { day: day.day })),
        hours: component.hours.map((hour) => bar(hour, { hour: hour.at })),
      })
    )
    return {
      id: group.id,
      name: group.name,
      defaultOpen: group.default_open,
      status: worst(components.map((component) => component.status)),
      components,
    }
  })

  const active = snapshot.incidents
    .filter((incident) => incident.state !== 'resolved')
    .map((incident) => incidentView(incident, names))

  const maintenance = snapshot.maintenance
    .map((window) => ({
      title: window.title,
      description: window.description,
      active: window.active,
      startsAt: window.starts_at,
      endsAt: window.ends_at,
      components: window.components.flatMap((id) => names.get(id) ?? []),
    }))
    // Under way first, then by start.
    .sort((a, b) => Number(b.active) - Number(a.active) || (a.startsAt ?? 0) - (b.startsAt ?? 0))

  const next = maintenance[0]

  return {
    title: snapshot.appearance.title,
    description: snapshot.description,
    footer: snapshot.footer,
    logo: snapshot.appearance.logo === undefined ? undefined : `${base}${snapshot.appearance.logo}`,
    status: snapshot.status,
    audience: snapshot.audience,
    groups,
    active,
    names,
    maintenance,
    slas: snapshot.slas.map((sla) => slaView(sla, names)),
    incidentSpans: snapshot.incidents.map((incident) => ({ startedAt: incident.started_at, resolvedAt: incident.resolved_at })),
    nextMaintenance: next?.active ? 'now' : next?.startsAt,
    rangeMax: snapshot.range_max,
    generatedAt: snapshot.generated_at,
  }
}
