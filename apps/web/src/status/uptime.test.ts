import { describe, expect, it } from 'vitest'

import { componentBasis, explain, explainDay, pageBasis } from './uptime'

const bar = (day: string, up = 0, down = 0, maint = 0, approximate = false) => ({ day, counts: { up, down, maint }, approximate })

describe('uptime basis', () => {
  it('covers a component from its first checked day and sums its checks', () => {
    const basis = componentBasis([bar('2026-07-01'), bar('2026-07-02', 90, 10), bar('2026-07-03'), bar('2026-07-04', 100, 0, 5, true)])
    expect(basis).toEqual({ counts: { up: 190, down: 10, maint: 5 }, from: '2026-07-02', to: '2026-07-04', days: 3, imported: true })
    expect(componentBasis([bar('2026-07-01')])).toMatchObject({ days: 0, from: undefined })
  })

  it('says the headline averages the components that have a figure', () => {
    const basis = pageBasis([
      { uptime: 1, bars: [bar('2026-07-01'), bar('2026-07-02', 10)] },
      { uptime: 0.9, bars: [bar('2026-07-01', 9, 1), bar('2026-07-02', 10)] },
      { bars: [bar('2026-07-01'), bar('2026-07-02')] },
    ])
    expect(basis).toMatchObject({ averaged: 2, counts: { up: 29, down: 1, maint: 0 }, from: '2026-07-01', days: 2 })
  })
})

describe('uptime explanation', () => {
  it('states the checks, the excluded maintenance, the period and the rules that apply', () => {
    const { facts, rules } = explain(componentBasis([bar('2026-07-02', 1190, 10, 5), bar('2026-07-03', 100)]))
    expect(facts).toEqual(['1,300 checks counted: 1,290 up, 10 down', '5 under maintenance, left out', 'Covers Jul 2, 2026 to Jul 3, 2026, 2 days'])
    expect(rules).toHaveLength(3)
    expect(explain(componentBasis([bar('2026-07-02', 1, 0, 0, true)])).rules).toHaveLength(4)
  })

  it('explains a day only as far as it needs explaining', () => {
    expect(explainDay({ up: 0, down: 0, maint: 0 })).toEqual({ facts: [], rules: [] })
    expect(explainDay({ up: 10, down: 0, maint: 0 })).toEqual({ facts: ['10 checks counted: 10 up, 0 down'], rules: [] })
    expect(explainDay({ up: 9, down: 1, maint: 0 }).rules).toHaveLength(2)
  })
})
