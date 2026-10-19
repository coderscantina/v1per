import { describe, expect, it } from 'vitest'

import type { AffectedTarget } from '~/generated'

import { changedImpacts, currentImpacts, parseTargetKey, selectTargets, targetKey, toImpacts, worstImpact } from './incidents'

const affected: AffectedTarget[] = [
  { target: { type: 'monitor', monitor: '3' }, name: 'API', impact: 'major_outage' },
  { target: { type: 'component', component: '8' }, name: 'Billing', impact: 'degraded' },
  { target: { type: 'component', component: '9' }, name: 'Docs' },
]

describe('impact selection', () => {
  it('names targets by one key either way', () => {
    expect(targetKey({ type: 'component', component: '8' })).toBe('component:8')
    expect(parseTargetKey('monitor:3')).toEqual({ type: 'monitor', monitor: '3' })
  })

  it('starts picked targets at a major outage and keeps the impact of those still picked', () => {
    const first = selectTargets({}, ['monitor:3', 'component:8'])
    expect(first).toEqual({ 'monitor:3': 'major_outage', 'component:8': 'major_outage' })
    const next = selectTargets({ ...first, 'component:8': 'degraded' }, ['component:8', 'monitor:5'])
    expect(next).toEqual({ 'component:8': 'degraded', 'monitor:5': 'major_outage' })
    expect(toImpacts(next)).toEqual([
      { target: { type: 'component', component: '8' }, impact: 'degraded' },
      { target: { type: 'monitor', monitor: '5' }, impact: 'major_outage' },
    ])
  })

  it('sends only the impacts an update changes, and none when it resolves', () => {
    const chosen = { ...currentImpacts(affected), 'component:8': 'partial_outage' as const, 'component:9': 'degraded' as const }
    expect(currentImpacts(affected)).toEqual({ 'monitor:3': 'major_outage', 'component:8': 'degraded' })
    expect(changedImpacts(affected, chosen, 'identified')).toEqual([
      { target: { type: 'component', component: '8' }, impact: 'partial_outage' },
      { target: { type: 'component', component: '9' }, impact: 'degraded' },
    ])
    expect(changedImpacts(affected, chosen, 'resolved')).toEqual([])
  })

  it('reads the worst impact', () => {
    expect(worstImpact(affected.map((entry) => entry.impact))).toBe('major_outage')
    expect(worstImpact(['degraded', undefined, 'operational'])).toBe('degraded')
    expect(worstImpact([])).toBeUndefined()
  })
})
