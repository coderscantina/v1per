import type { AffectedTarget, Impact, IncidentState, Target, TargetImpact } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/**
 * Incident impact and state (monitoring contract, incident impact). An update sets the impact
 * of the targets it names; the latest update that names a target wins, and resolving sets every
 * target to operational on the server.
 */

/** Best to worst. */
export const impacts = ['operational', 'degraded', 'partial_outage', 'major_outage'] as const satisfies readonly Impact[]
export const incidentStates = ['investigating', 'identified', 'monitoring', 'resolved'] as const satisfies readonly IncidentState[]

/** A person filing an incident starts where an auto-incident does. */
export const DEFAULT_IMPACT: Impact = 'major_outage'

export const impactLabels: Record<Impact, MessageKey> = {
  operational: 'incidents.impacts.operational',
  degraded: 'incidents.impacts.degraded',
  partial_outage: 'incidents.impacts.partial_outage',
  major_outage: 'incidents.impacts.major_outage',
}

/** The dot colour. A word always goes with it. */
export const impactFill: Record<Impact, string> = {
  operational: 'bg-status-up',
  degraded: 'bg-status-pending',
  partial_outage: 'bg-status-partial',
  major_outage: 'bg-status-down',
}

export const stateLabels: Record<IncidentState, MessageKey> = {
  investigating: 'incidents.states.investigating',
  identified: 'incidents.states.identified',
  monitoring: 'incidents.states.monitoring',
  resolved: 'incidents.states.resolved',
}

export const impactRank = (impact: Impact): number => impacts.indexOf(impact)

/** The worst of several impacts; `undefined` for none. */
export const worstImpact = (list: readonly (Impact | undefined)[]): Impact | undefined =>
  list.reduce<Impact | undefined>(
    (worst, impact) => (impact !== undefined && (worst === undefined || impactRank(impact) > impactRank(worst)) ? impact : worst),
    undefined
  )

/** A target as one string, for maps, checkbox values and keys: `monitor:12` or `component:3`. */
export type TargetKey = `monitor:${string}` | `component:${string}`

export const targetKey = (target: Target): TargetKey =>
  target.type === 'monitor' ? `monitor:${target.monitor}` : `component:${target.component}`

export function parseTargetKey(key: TargetKey): Target {
  const [type, id = ''] = key.split(':') as ['monitor' | 'component', string]
  return type === 'monitor' ? { type: 'monitor', monitor: id } : { type: 'component', component: id }
}

/** The impact chosen per target, in the order the targets were picked. */
export type ImpactSelection = Partial<Record<TargetKey, Impact>>

/** Picks or drops targets: newly picked ones start at `DEFAULT_IMPACT`, kept ones keep theirs. */
export function selectTargets(selection: ImpactSelection, picked: readonly TargetKey[]): ImpactSelection {
  const next: ImpactSelection = {}
  for (const key of picked) next[key] = selection[key] ?? DEFAULT_IMPACT
  return next
}

/** `incidents.create`'s `impacts`: every picked target once, with its impact. */
export const toImpacts = (selection: ImpactSelection): TargetImpact[] =>
  (Object.entries(selection) as Array<[TargetKey, Impact | undefined]>)
    .filter((entry): entry is [TargetKey, Impact] => entry[1] !== undefined)
    .map(([key, impact]) => ({ target: parseTargetKey(key), impact }))

/** The update form's starting point: each affected target at its current impact, when it has one. */
export const currentImpacts = (affected: readonly AffectedTarget[]): ImpactSelection =>
  Object.fromEntries(affected.flatMap((entry) => (entry.impact ? [[targetKey(entry.target), entry.impact]] : [])))

/**
 * `incidents.addUpdate`'s `impacts`: only the targets whose impact the person changed, so the
 * update names what it is about. Resolving names none; the server sets every target to
 * operational.
 */
export function changedImpacts(affected: readonly AffectedTarget[], chosen: ImpactSelection, state: IncidentState): TargetImpact[] {
  if (state === 'resolved') return []
  return affected.flatMap((entry) => {
    const impact = chosen[targetKey(entry.target)]
    return impact !== undefined && impact !== entry.impact ? [{ target: entry.target, impact }] : []
  })
}
