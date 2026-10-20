import type { DayBar, HeartbeatStatus, MonitorType, Quorum } from '~/generated'
import { formatNumber, type NumberLocale } from '~/lib/formats'
import type { IconName } from '~/lib/icons'
import type { MessageKey } from '~/plugins/i18n'

/**
 * How a monitor reads at a glance: its latest status, `paused` while inactive whatever it last
 * reported, `unknown` before its first beat (monitoring contract, heartbeat status).
 */
export type Presence = HeartbeatStatus | 'paused' | 'unknown'

export const presenceOf = (monitor: { active: boolean; status?: HeartbeatStatus }): Presence =>
  monitor.active ? (monitor.status ?? 'unknown') : 'paused'

/** Worst first, so a list sorted by it shows what needs attention at the top. */
export const presences = ['down', 'pending', 'maintenance', 'up', 'unknown', 'paused'] as const satisfies readonly Presence[]

export const presenceLabels: Record<Presence, MessageKey> = {
  down: 'monitors.status.down',
  pending: 'monitors.status.pending',
  maintenance: 'monitors.status.maintenance',
  up: 'monitors.status.up',
  unknown: 'monitors.status.unknown',
  paused: 'monitors.status.paused',
}

/** The dot or bar colour. A word always goes with it, so colour is never the only cue. */
export const presenceFill: Record<Presence, string> = {
  down: 'bg-status-down',
  pending: 'bg-status-pending',
  maintenance: 'bg-status-maintenance',
  up: 'bg-status-up',
  unknown: 'bg-status-none',
  paused: 'bg-status-none',
}

/** The same colours as values, for SVG strokes and gradient stops. */
export const presenceColor: Record<Presence, string> = {
  down: 'var(--status-down)',
  pending: 'var(--status-pending)',
  maintenance: 'var(--status-maintenance)',
  up: 'var(--status-up)',
  unknown: 'var(--status-none)',
  paused: 'var(--status-none)',
}

export const presenceRank = (presence: Presence): number => presences.indexOf(presence)

/** Counts per presence, for the node overview's summary. */
export function countPresences(monitors: Array<{ active: boolean; status?: HeartbeatStatus }>): Record<Presence, number> {
  const counts: Record<Presence, number> = { down: 0, pending: 0, maintenance: 0, up: 0, unknown: 0, paused: 0 }
  for (const monitor of monitors) counts[presenceOf(monitor)] += 1
  return counts
}

export const typeLabels: Record<MonitorType, MessageKey> = {
  http: 'monitors.types.http',
  keyword: 'monitors.types.keyword',
  'json-query': 'monitors.types.json-query',
  port: 'monitors.types.port',
  ping: 'monitors.types.ping',
  dns: 'monitors.types.dns',
  push: 'monitors.types.push',
  group: 'monitors.types.group',
  manual: 'monitors.types.manual',
  unsupported: 'monitors.types.unsupported',
}

export const typeIcons: Record<MonitorType, IconName> = {
  http: 'lucide:globe',
  keyword: 'lucide:text-search',
  'json-query': 'lucide:braces',
  port: 'lucide:plug',
  ping: 'lucide:radar',
  dns: 'lucide:signpost',
  push: 'lucide:webhook',
  group: 'lucide:folder-tree',
  manual: 'lucide:hand',
  unsupported: 'lucide:circle-slash',
}

export const quorumLabels: Record<Quorum, MessageKey> = {
  majority: 'monitors.editor.quorums.majority',
  any: 'monitors.editor.quorums.any',
  all: 'monitors.editor.quorums.all',
}

/**
 * Uptime as a percentage with two decimals, as Kuma shows it. Rounded down, so a window with a
 * single miss never reads as 100 %. `null` without up or down beats in the window.
 */
export function formatUptime(ratio: number | undefined, locale: NumberLocale): string | null {
  if (ratio === undefined) return null
  const floored = Math.floor(ratio * 10_000) / 10_000
  const digits = floored >= 1 ? 0 : 2
  return formatNumber(locale, floored, { style: 'percent', minimumFractionDigits: digits, maximumFractionDigits: digits })
}

/** Milliseconds, with one decimal below 10 ms where a whole number would hide the difference. */
export const formatLatency = (ms: number, locale: NumberLocale): string =>
  `${formatNumber(locale, ms, { maximumFractionDigits: ms < 10 ? 1 : 0 })} ms`

/**
 * A bar's colour, for a day or a slot of the monitor page's history: all up, degraded from 95 %
 * uptime, down below it. Pending counts as down, as in the uptime number. A bar with only
 * maintenance shows as maintenance, one without beats as empty. This follows the contract's
 * proposed bar rule, coarsened to three tones.
 */
export type BarTone = 'up' | 'degraded' | 'down' | 'maintenance' | 'empty'

export function barTone(bar: Pick<DayBar, 'uptime' | 'maintenance'>): BarTone {
  if (bar.uptime === undefined) return bar.maintenance > 0 ? 'maintenance' : 'empty'
  if (bar.uptime >= 1) return 'up'
  return bar.uptime >= 0.95 ? 'degraded' : 'down'
}

export const barFill: Record<BarTone, string> = {
  up: 'bg-status-up',
  degraded: 'bg-status-pending',
  down: 'bg-status-down',
  maintenance: 'bg-status-maintenance',
  empty: 'bg-fill-strong',
}
