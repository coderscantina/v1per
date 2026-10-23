import type {
  Impact,
  Locale,
  PageSla,
  PageSlaAvailability,
  PageSlaClock,
  PageSlaLatency,
  PageSlaPeriod,
  SlaCombine,
  SlaOutcome,
  SlaPeriodKind,
} from '~/generated'

import { formatDuration, formatLatency, formatUptime } from './format'
import { t, type MessageKey } from './messages'

/**
 * How a commitment or period reads on the page. `at_risk` is a running `on_track` that would
 * miss if the period ended now: 75 % of the downtime budget used, the warning the SLA alerts
 * send, or an average latency above its target.
 */
export type SlaState = SlaOutcome | 'at_risk'

/** The share of the downtime budget from which a running period reads at risk. */
export const AT_RISK_BUDGET = 0.75

export interface AvailabilityView {
  state: SlaState
  /** Percent. */
  target: number
  /** Percent: the service in `any_down` mode, the lowest monitor in `each` mode. */
  actual?: number
  /** Share of the downtime budget, 1 being all of it. */
  budgetUsed?: number
}

export interface LatencyView {
  state: SlaState
  targetMs: number
  actualMs?: number
}

export interface ClockView {
  state: SlaState
  /** Business minutes per impact, worst impact first. */
  targets: { impact: Impact; minutes: number }[]
  counted: number
  met: number
  withinTarget?: number
  averageMs?: number
}

export interface SlaMonitorView {
  name: string
  availability?: number
  availabilityState?: SlaState
  latencyMs?: number
  latencyState?: SlaState
}

export interface SlaPeriodView {
  start: string
  startsAt: number
  endsAt: number
  state: SlaState
  availability?: AvailabilityView
  latency?: LatencyView
  response?: ClockView
  resolution?: ClockView
  /** Grant holders only, named by the page components they show as. */
  monitors: SlaMonitorView[]
}

export interface SlaView {
  name: string
  timezone: string
  period: SlaPeriodKind
  combine: SlaCombine
  current?: SlaPeriodView
  /** Closed periods, oldest first, so they read left to right. */
  history: SlaPeriodView[]
}

function availabilityView(availability: PageSlaAvailability): AvailabilityView {
  const atRisk = availability.outcome === 'on_track' && (availability.budget_used ?? 0) >= AT_RISK_BUDGET
  return {
    state: atRisk ? 'at_risk' : availability.outcome,
    target: availability.target,
    actual: availability.actual,
    budgetUsed: availability.budget_used,
  }
}

function latencyState(outcome: SlaOutcome, actualMs: number | undefined, targetMs: number): SlaState {
  return outcome === 'on_track' && actualMs !== undefined && actualMs > targetMs ? 'at_risk' : outcome
}

const impacts = ['major_outage', 'partial_outage', 'degraded'] as const satisfies readonly Impact[]

function clockView(clock: PageSlaClock): ClockView {
  return {
    state: clock.outcome,
    targets: impacts.flatMap((impact) => {
      const minutes = clock.targets[impact]
      return minutes === undefined ? [] : [{ impact, minutes }]
    }),
    counted: clock.counted,
    met: clock.met,
    withinTarget: clock.within_target,
    averageMs: clock.average_ms,
  }
}

function latencyView(latency: PageSlaLatency): LatencyView {
  return {
    state: latencyState(latency.outcome, latency.actual_ms, latency.target_ms),
    targetMs: latency.target_ms,
    actualMs: latency.actual_ms,
  }
}

/** A period as the page draws it; monitors whose component the page does not name are left out. */
export function slaPeriodView(period: PageSlaPeriod, names: ReadonlyMap<string, string>): SlaPeriodView {
  const availability = period.availability && availabilityView(period.availability)
  const latency = period.latency && latencyView(period.latency)
  const atRisk = period.outcome === 'on_track' && (availability?.state === 'at_risk' || latency?.state === 'at_risk')
  return {
    start: period.start,
    startsAt: period.starts_at,
    endsAt: period.ends_at,
    state: atRisk ? 'at_risk' : period.outcome,
    availability,
    latency,
    response: period.response && clockView(period.response),
    resolution: period.resolution && clockView(period.resolution),
    monitors: (period.monitors ?? []).flatMap((monitor) => {
      const name = names.get(monitor.component)
      if (name === undefined) return []
      return [
        {
          name,
          availability: monitor.availability,
          availabilityState: monitor.availability_outcome,
          latencyMs: monitor.latency_ms,
          latencyState:
            monitor.latency_outcome && latency
              ? latencyState(monitor.latency_outcome, monitor.latency_ms, latency.targetMs)
              : monitor.latency_outcome,
        },
      ]
    }),
  }
}

export function slaView(sla: PageSla, names: ReadonlyMap<string, string>): SlaView {
  return {
    name: sla.name,
    timezone: sla.timezone,
    period: sla.period,
    combine: sla.combine,
    current: sla.current && slaPeriodView(sla.current, names),
    history: sla.history.map((period) => slaPeriodView(period, names)).reverse(),
  }
}

/** A period's name from its first day, as "October 2026", "Q4 2026" or "2026". */
export function formatPeriod(start: string, kind: SlaPeriodKind, locale: Locale, short = false): string {
  const [year = 0, month = 1] = start.split('-').map(Number)
  if (kind === 'year') return String(year)
  if (kind === 'quarter') return t('sla.quarter', { quarter: Math.floor((month - 1) / 3) + 1, year })
  return new Intl.DateTimeFormat(locale, { month: short ? 'short' : 'long', year: 'numeric', timeZone: 'UTC' }).format(
    Date.UTC(year, month - 1, 1)
  )
}

/** A percentage given in percent, rounded rather than floored: a target is exact. */
export function formatTarget(percent: number, locale: Locale): string {
  return new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 3 }).format(percent / 100)
}

/** A share such as the budget used or the incidents within target, in whole percent. */
export function formatShare(ratio: number, locale: Locale): string {
  return new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 0 }).format(ratio)
}

/** A time target in whole days or hours where it has no remainder, as "4 h" rather than "4 h 0 min". */
export function formatMinutes(minutes: number): string {
  if (minutes >= 1440 && minutes % 1440 === 0) return t('duration.wholeDays', { d: minutes / 1440 })
  if (minutes >= 60 && minutes % 60 === 0) return t('duration.wholeHours', { h: minutes / 60 })
  return formatDuration(minutes * 60_000)
}

/** One commitment of a period in a sentence, for the history's popover and labels. */
export interface PeriodLine {
  state: SlaState
  text: string
}

function clockLine(label: MessageKey, clock: ClockView): string {
  const name = t(label)
  if (clock.counted === 0) return t('sla.line.noIncidents', { name })
  return t('sla.line.clock', { name, met: clock.met, counted: clock.counted })
}

/** The commitments of a period, each with its state and figures, in the card's order. */
export function periodLines(period: SlaPeriodView, locale: Locale): PeriodLine[] {
  const lines: PeriodLine[] = []
  const { availability, latency, response, resolution } = period
  if (availability) {
    const name = t('sla.availability')
    const target = formatTarget(availability.target, locale)
    lines.push({
      state: availability.state,
      text:
        availability.actual === undefined
          ? t('sla.line.noData', { name, target })
          : t('sla.line.figure', { name, value: formatUptime(availability.actual / 100, locale), target }),
    })
  }
  if (latency) {
    const name = t('sla.latency')
    const target = formatLatency(latency.targetMs, locale)
    lines.push({
      state: latency.state,
      text:
        latency.actualMs === undefined
          ? t('sla.line.noData', { name, target })
          : t('sla.line.figure', { name, value: formatLatency(latency.actualMs, locale), target }),
    })
  }
  if (response) lines.push({ state: response.state, text: clockLine('sla.response', response) })
  if (resolution) lines.push({ state: resolution.state, text: clockLine('sla.resolution', resolution) })
  return lines
}
