import type { SlaClockOutcome, SlaOutcome, SlaPeriod, SlaPeriodKind } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/**
 * Helpers for one SLA period: which periods exist, how outcomes read, and the CSV export. The
 * period calendar mirrors the server's (`v1per-sla`): calendar months, quarters or years in the
 * SLA's timezone, the first one starting on the contract start date.
 */

/** A commitment's or an incident clock's outcome; both enums share `met` and `missed`. */
export type SlaStanding = SlaOutcome | SlaClockOutcome

export const standingLabels: Record<SlaStanding, MessageKey> = {
  met: 'slas.outcome.met',
  missed: 'slas.outcome.missed',
  on_track: 'slas.outcome.on_track',
  at_risk: 'slas.outcome.at_risk',
  no_data: 'slas.outcome.no_data',
  running: 'slas.outcome.running',
  no_target: 'slas.outcome.no_target',
  not_needed: 'slas.outcome.not_needed',
}

/** Badge tones: only a miss is red, at risk is amber, and no data stays neutral. */
export const standingTones = {
  met: 'success',
  missed: 'destructive',
  on_track: 'secondary',
  at_risk: 'warning',
  no_data: 'secondary',
  running: 'warning',
  no_target: 'secondary',
  not_needed: 'secondary',
} as const satisfies Record<SlaStanding, string>

const MONTHS: Record<SlaPeriodKind, number> = { month: 1, quarter: 3, year: 12 }

const ymd = (year: number, month: number, day: number) =>
  `${String(year).padStart(4, '0')}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`

/** The first of the month, quarter or year holding `day` (`YYYY-MM-DD`). */
function calendarFirst(kind: SlaPeriodKind, day: string): { year: number; month: number } {
  const year = Number(day.slice(0, 4))
  const month = Number(day.slice(5, 7))
  if (kind === 'year') return { year, month: 1 }
  if (kind === 'quarter') return { year, month: Math.floor((month - 1) / 3) * 3 + 1 }
  return { year, month }
}

/**
 * The first days of an SLA's periods from its start date through the one holding `today`,
 * newest first. Before the contract starts, the first period alone.
 */
export function periodStarts(kind: SlaPeriodKind, startDate: string, today: string): string[] {
  const starts = [startDate]
  let { year, month } = calendarFirst(kind, startDate)
  for (;;) {
    month += MONTHS[kind]
    if (month > 12) {
      year += 1
      month -= 12
    }
    const next = ymd(year, month, 1)
    if (next > today) break
    starts.push(next)
  }
  return starts.reverse()
}

/** Today as `YYYY-MM-DD` in `timeZone`. */
export function todayIn(timeZone: string, now = Date.now()): string {
  const parts = new Intl.DateTimeFormat('en-US', { year: 'numeric', month: '2-digit', day: '2-digit', timeZone }).formatToParts(now)
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((entry) => entry.type === type)?.value ?? ''
  return `${part('year')}-${part('month')}-${part('day')}`
}

/** Whether a monitor's outcome is worth a badge: a result or a warning, not the running state or no data. */
export const settled = (standing: SlaStanding | undefined): standing is 'met' | 'missed' | 'at_risk' =>
  standing === 'met' || standing === 'missed' || standing === 'at_risk'

/** A period's name from its first day: "October 2026", "Q4 2026" or "2026". */
export function periodTitle(kind: SlaPeriodKind, start: string, language: string): string {
  const { year, month } = calendarFirst(kind, start)
  if (kind === 'year') return String(year)
  if (kind === 'quarter') return `Q${Math.floor((month - 1) / 3) + 1} ${year}`
  return new Intl.DateTimeFormat(language, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(Date.UTC(year, month - 1, 1))
}

/** The highest average latency among the covered monitors, or `null` without any. */
export function highestLatency(period: Pick<SlaPeriod, 'monitors'>): number | null {
  const values = period.monitors.flatMap((entry) => (entry.latency_ms === undefined ? [] : [entry.latency_ms]))
  return values.length > 0 ? Math.max(...values) : null
}

export type CsvCell = string | number | null | undefined

/**
 * Rows as RFC 4180 CSV with CRLF line ends. A field with a comma, quote or line break is quoted
 * with its quotes doubled. Text that a spreadsheet would run as a formula (`=`, `+`, `-`, `@`,
 * tab, carriage return first) gets a leading apostrophe; numbers are written as they are.
 */
export function toCsv(rows: readonly (readonly CsvCell[])[]): string {
  const field = (cell: CsvCell): string => {
    if (cell === null || cell === undefined) return ''
    if (typeof cell === 'number') return Number.isFinite(cell) ? String(cell) : ''
    const text = /^[=+\-@\t\r]/.test(cell) ? `'${cell}` : cell
    return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text
  }
  return rows.map((row) => `${row.map(field).join(',')}\r\n`).join('')
}

const round = (value: number | undefined, digits: number) =>
  value === undefined ? null : Math.round(value * 10 ** digits) / 10 ** digits
const iso = (ms: number | undefined) => (ms === undefined ? null : new Date(ms).toISOString())
const minutes = (ms: number | undefined) => (ms === undefined ? null : round(ms / 60_000, 1))

/**
 * A period as CSV: the covered monitors, a blank line, then the incidents. Values stay machine
 * readable: percentages and milliseconds with a decimal point, times in ISO 8601 UTC, outcomes
 * as their protocol names. Incident times are business minutes.
 */
export function slaPeriodCsv(period: SlaPeriod): string {
  const monitors = period.monitors.map((entry) => [
    entry.monitor,
    entry.name,
    round(entry.availability, 4),
    entry.availability_outcome,
    entry.budget_used === undefined ? null : round(entry.budget_used * 100, 1),
    round(entry.latency_ms, 1),
    entry.latency_outcome,
  ])
  const incidents = period.incidents.map((entry) => [
    entry.incident,
    entry.title,
    entry.impact,
    iso(entry.started_at),
    iso(entry.resolved_at),
    entry.response.outcome,
    minutes(entry.response.elapsed_ms),
    iso(entry.response.deadline),
    entry.resolution.outcome,
    minutes(entry.resolution.elapsed_ms),
    iso(entry.resolution.deadline),
  ])
  return toCsv([
    ['monitor_id', 'monitor', 'availability_percent', 'availability_outcome', 'budget_used_percent', 'latency_ms', 'latency_outcome'],
    ...monitors,
    [],
    [
      'incident_id',
      'incident',
      'impact',
      'started_at',
      'resolved_at',
      'response_outcome',
      'response_minutes',
      'response_deadline',
      'resolution_outcome',
      'resolution_minutes',
      'resolution_deadline',
    ],
    ...incidents,
  ])
}

/** A file name for the period's CSV: the SLA name in lowercase words, then the first day. */
export const csvFileName = (name: string, start: string) =>
  `${name.toLocaleLowerCase().replaceAll(/[^\p{L}\p{N}]+/gu, '-').replaceAll(/^-|-$/g, '') || 'sla'}-${start}.csv`
