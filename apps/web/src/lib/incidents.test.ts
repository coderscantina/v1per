import { describe, expect, it } from 'vitest'

import type { AffectedTarget } from '~/generated'

import {
  allAt,
  changedImpacts,
  currentImpacts,
  durationParts,
  openFirst,
  parseTargetKey,
  selectTargets,
  sharedImpact,
  targetKey,
  toImpacts,
  worstImpact,
} from './incidents'

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

  it('starts new targets at the impact the composer shows and reads one shared impact', () => {
    expect(selectTargets({ 'monitor:3': 'degraded' }, ['monitor:3', 'component:8'], 'partial_outage')).toEqual({
      'monitor:3': 'degraded',
      'component:8': 'partial_outage',
    })
    expect(sharedImpact(allAt(['monitor:3', 'component:8'], 'degraded'))).toBe('degraded')
    expect(sharedImpact({ 'monitor:3': 'degraded', 'component:8': 'major_outage' })).toBeUndefined()
    expect(sharedImpact({})).toBeUndefined()
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

describe('incident feed', () => {
  it('lists open incidents before resolved ones, keeping their order', () => {
    const list = [
      { id: 'a', state: 'resolved' as const },
      { id: 'b', state: 'monitoring' as const },
      { id: 'c', state: 'resolved' as const },
      { id: 'd', state: 'investigating' as const },
    ]
    expect(openFirst(list).map((entry) => entry.id)).toEqual(['b', 'd', 'a', 'c'])
  })

  it('reads a span as its two largest units', () => {
    expect(durationParts(20_000)).toEqual([['minute', 1]])
    expect(durationParts((2 * 60 + 5) * 60_000)).toEqual([
      ['hour', 2],
      ['minute', 5],
    ])
    expect(durationParts((3 * 1440 + 30) * 60_000)).toEqual([['day', 3]])
  })
})
