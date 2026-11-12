import type { SlaOutcome, SlaPeriod, SlaTimeTarget } from '~/generated'
import { formatNumber } from '~/lib/formats'
import { impactLabels } from '~/lib/incidents'
import { durationParts, slaImpacts } from '~/lib/sla-form'
import { highestLatency, periodTitle } from '~/lib/sla-period'
import { useI18n } from '~/plugins/i18n'

import { useDateFormat } from './useDateFormat'
import { useFormats } from './usePreferences'

export type CommitmentKey = 'availability' | 'latency' | 'response' | 'resolution'

/** One commitment of a period as text, for the period view and the report. */
export interface Commitment {
  key: CommitmentKey
  label: string
  target: string
  /** The figure, or `null` when the data cannot support it. */
  actual: string | null
  /** What the figure is, such as "lowest monitor", and for incidents the counts and average. */
  notes: string[]
  outcome: SlaOutcome
  /** Share of the downtime budget used, 1 being all of it. Availability only. */
  budget?: number
}

/**
 * SLA figures in the reader's formats: percentages, latencies, business durations, the period's
 * name and dates in the SLA's timezone, and the commitments of a period.
 */
export function useSlaFormat() {
  const { t, locale } = useI18n()
  const { numberLocale } = useFormats()
  const { date } = useDateFormat()

  const percent = (value: number) => `${formatNumber(numberLocale.value, value, { maximumFractionDigits: 3 })} %`
  const share = (fraction: number) => `${formatNumber(numberLocale.value, fraction * 100, { maximumFractionDigits: 0 })} %`
  const ms = (value: number) => `${formatNumber(numberLocale.value, value, { maximumFractionDigits: 0 })} ms`
  /** Whole minutes as "4 h" or "1 h 30 min", as the SLA list shows them. */
  const minutes = (total: number) => {
    const parts = durationParts(total)
    return [parts.hours > 0 ? t('slas.units.hours', { count: parts.hours }) : null, parts.minutes > 0 ? t('slas.units.minutes', { count: parts.minutes }) : null]
      .filter((part) => part !== null)
      .join(' ')
  }
  /** A span in milliseconds as whole minutes, or "under 1 min" below one minute. */
  const duration = (span: number) => (span < 60_000 ? t('slas.units.underMinute') : minutes(Math.floor(span / 60_000)))

  const title = (period: Pick<SlaPeriod, 'start' | 'terms'>) => periodTitle(period.terms.period, period.start, locale.value)
  /** First and last day in the SLA's timezone. */
  const range = (period: SlaPeriod) =>
    t('slas.view.range', {
      from: date(period.starts_at, period.terms.timezone),
      to: date(period.ends_at - 1, period.terms.timezone),
      zone: period.terms.timezone,
    })

  /** Time targets of one clock per impact, "Major outage 15m · Degraded 1h". */
  const timeTargets = (period: SlaPeriod, clock: keyof SlaTimeTarget) =>
    slaImpacts
      .flatMap((impact) => {
        const target = period.terms.times?.[impact]?.[clock]
        return target === undefined ? [] : [`${t(impactLabels[impact])} ${duration(target * 60_000)}`]
      })
      .join(' · ')

  const commitments = (period: SlaPeriod): Commitment[] => {
    const rows: Commitment[] = []
    const { availability, latency, response, resolution, terms } = period
    if (availability && terms.availability !== undefined) {
      rows.push({
        key: 'availability',
        label: t('slas.view.availability'),
        target: percent(terms.availability),
        actual: availability.actual === undefined ? null : percent(availability.actual),
        notes: [terms.combine === 'each' ? t('slas.view.lowest') : t('slas.view.service')],
        outcome: availability.outcome,
        budget: availability.budget_used,
      })
    }
    if (latency && terms.latency_ms !== undefined) {
      const highest = highestLatency(period)
      rows.push({
        key: 'latency',
        label: t('slas.view.latency'),
        target: ms(terms.latency_ms),
        actual: highest === null ? null : ms(highest),
        notes: [t('slas.view.highest'), ...(period.ends_at > Date.now() ? [t('slas.view.latencyAtEnd')] : [])],
        outcome: latency,
      })
    }
    for (const [key, summary, clock] of [
      ['response', response, 'response_minutes'],
      ['resolution', resolution, 'resolution_minutes'],
    ] as const) {
      if (!summary) continue
      rows.push({
        key,
        label: t(`slas.view.${key}`),
        target: timeTargets(period, clock),
        actual: summary.counted > 0 ? share(summary.met / summary.counted) : t('slas.view.noneCounted'),
        notes: [
          ...(summary.counted > 0 ? [t('slas.view.withinTarget', { met: summary.met, counted: summary.counted })] : []),
          ...(summary.average_ms === undefined ? [] : [t('slas.view.average', { time: duration(summary.average_ms) })]),
        ],
        outcome: summary.outcome,
      })
    }
    return rows
  }

  return { percent, share, ms, minutes, duration, title, range, commitments }
}
