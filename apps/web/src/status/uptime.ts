import type { PageBar } from '~/generated'

import { formatDay } from './format'
import { locale, t, type MessageKey } from './messages'

/**
 * How the page's uptime figures were computed, from the checks the snapshot carries, in the
 * monitoring contract's terms: uptime is up checks over up and down checks, retrying checks count
 * as down, checks under maintenance are left out, and a monitor with several probes is down by
 * its quorum. The headline is the plain average of the components' figures.
 */

/** Monitor-level checks; `down` includes the ones still retrying. */
export interface Counts {
  up: number
  down: number
  maint: number
}

/** The counts of a bar or component on the wire, where zeros are left out. */
export function countsOf(wire: Pick<PageBar, 'up' | 'down' | 'maint'>): Counts {
  return { up: wire.up ?? 0, down: wire.down ?? 0, maint: wire.maint ?? 0 }
}

/** What one uptime figure stands on. */
export interface UptimeBasis {
  counts: Counts
  /** First and last day covered, `YYYY-MM-DD`; absent without any check. */
  from?: string
  to?: string
  /** Days from `from` to `to`, both counted. */
  days: number
  /** Some day came from an imported Uptime Kuma daily row, counted by UTC day. */
  imported: boolean
  /** For the headline: how many component figures it averages. */
  averaged?: number
}

interface BarLike {
  day: string
  counts: Counts
  approximate: boolean
}

const checked = (counts: Counts) => counts.up + counts.down + counts.maint > 0

/** The basis of a component's 90-day figure: from its first checked day to today. */
export function componentBasis(bars: readonly BarLike[]): UptimeBasis {
  const first = bars.findIndex((bar) => checked(bar.counts))
  const covered = first === -1 ? [] : bars.slice(first)
  const counts = covered.reduce(
    (sum, bar) => ({ up: sum.up + bar.counts.up, down: sum.down + bar.counts.down, maint: sum.maint + bar.counts.maint }),
    { up: 0, down: 0, maint: 0 }
  )
  return {
    counts,
    from: covered[0]?.day,
    to: covered.at(-1)?.day,
    days: covered.length,
    imported: covered.some((bar) => bar.approximate),
  }
}

/** The basis of the headline: the figures of the components that have one, averaged. */
export function pageBasis(components: readonly { uptime?: number; bars: readonly BarLike[] }[]): UptimeBasis {
  const bases = components.filter((component) => component.uptime !== undefined).map((component) => componentBasis(component.bars))
  const days = bases.flatMap((basis) => (basis.from && basis.to ? [basis] : []))
  const longest = days.reduce<UptimeBasis | undefined>((best, basis) => (best && best.days >= basis.days ? best : basis), undefined)
  return {
    counts: bases.reduce(
      (sum, basis) => ({ up: sum.up + basis.counts.up, down: sum.down + basis.counts.down, maint: sum.maint + basis.counts.maint }),
      { up: 0, down: 0, maint: 0 }
    ),
    from: longest?.from,
    to: longest?.to,
    days: longest?.days ?? 0,
    imported: bases.some((basis) => basis.imported),
    averaged: bases.length,
  }
}

/** An explanation as the page shows it: facts about the figure, then the rules behind it. */
export interface Explanation {
  facts: string[]
  rules: string[]
}

function count(value: number): string {
  return new Intl.NumberFormat(locale.value).format(value)
}

const plurals = {
  days: { one: 'uptime.days.one', other: 'uptime.days.other' },
  average: { one: 'uptime.average.one', other: 'uptime.average.other' },
} satisfies Record<string, Record<'one' | 'other', MessageKey>>

/** The singular or plural text for `n` in the current locale. */
function plural(forms: Record<'one' | 'other', MessageKey>, n: number, params: Record<string, string> = {}): string {
  const form = new Intl.PluralRules(locale.value).select(n) === 'one' ? 'one' : 'other'
  return t(forms[form], { n: count(n), ...params })
}

function checks(counts: Counts): string[] {
  const total = counts.up + counts.down
  const lines = [
    total === 0 ? t('uptime.noChecks') : t('uptime.checks', { total: count(total), up: count(counts.up), down: count(counts.down) }),
  ]
  if (counts.maint > 0) lines.push(t('uptime.maintenance', { n: count(counts.maint) }))
  return lines
}

/** A component's or the page's figure: what it counted over which days, and every rule. */
export function explain(basis: UptimeBasis): Explanation {
  const facts = checks(basis.counts)
  if (basis.from && basis.to) {
    const from = formatDay(basis.from, locale.value)
    const to = formatDay(basis.to, locale.value)
    facts.push(plural(plurals.days, basis.days, { from, to }))
  }
  if (basis.averaged !== undefined) facts.push(plural(plurals.average, basis.averaged))
  const rules = [t('uptime.rule.down'), t('uptime.rule.maintenance'), t('uptime.rule.quorum')]
  if (basis.imported) rules.push(t('uptime.rule.imported'))
  return { facts, rules }
}

/** One day: its checks, and the rules behind a day that was not all up. */
export function explainDay(counts: Counts): Explanation {
  return {
    facts: checked(counts) ? checks(counts) : [],
    rules: counts.down > 0 ? [t('uptime.rule.down'), t('uptime.rule.quorum')] : [],
  }
}

/** Every rule at once, for the page's "How uptime is counted". */
export function rulesInFull(): string[] {
  return [
    t('uptime.rule.ratio'),
    t('uptime.rule.down'),
    t('uptime.rule.maintenance'),
    t('uptime.rule.quorum'),
    t('uptime.rule.imported'),
    t('uptime.rule.bars'),
    t('uptime.rule.headline'),
  ]
}
