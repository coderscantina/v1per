import { describe, expect, it } from 'vitest'

import type { PageBar, PageSnapshot, PublicComponent } from '~/generated'

import { formatUptime } from './format'
import { pageView, worst } from './view'

const day = (index: number): PageBar => ({ day: `2026-07-${String(index + 1).padStart(2, '0')}` })

function component(id: string, name: string, extra: Partial<PublicComponent> = {}): PublicComponent {
  return { id, name, description: '', bars: [day(0), day(1)], ...extra }
}

function snapshot(extra: Partial<PageSnapshot> = {}): PageSnapshot {
  return {
    appearance: { title: 'Acme status', theme: 'auto', accent: 'blue', default_locale: 'en', locales: ['en'] },
    description: '',
    footer: '',
    timezone: 'Europe/Vienna',
    locale: 'en',
    audience: 'public',
    status: 'operational',
    groups: [],
    incidents: [],
    maintenance: [],
    cursor: 'page:1@0',
    generated_at: 0,
    ...extra,
  }
}

describe('pageView', () => {
  it('resolves bar incidents to titles and marks days without data', () => {
    const view = pageView(
      snapshot({
        groups: [
          {
            id: 'g1',
            name: 'Services',
            default_open: true,
            components: [
              component('c1', 'API', {
                status: 'major_outage',
                bars: [
                  { day: '2026-07-01' },
                  { day: '2026-07-02', status: 'partial_outage', uptime: 0.97, incidents: ['i1'], utc_approximate: true },
                ],
              }),
              component('c2', 'Email'),
            ],
          },
        ],
        incidents: [
          { id: 'i1', title: 'API is down', state: 'resolved', started_at: 0, resolved_at: 1, components: ['c1'], updates: [] },
        ],
      }),
      '/s/acme'
    )
    const [api, email] = view.groups[0]?.components ?? []
    expect(api?.bars).toEqual([
      { day: '2026-07-01', status: 'none', uptime: undefined, approximate: false, maintenance: false, incidents: [] },
      { day: '2026-07-02', status: 'partial_outage', uptime: 0.97, approximate: true, maintenance: false, incidents: ['API is down'] },
    ])
    expect(email?.status).toBe('none')
    expect(view.groups[0]?.status).toBe('major_outage')
  })

  it('keeps open incidents and takes each impact from the newest update naming it', () => {
    const view = pageView(
      snapshot({
        groups: [{ id: 'g', name: '', default_open: true, components: [component('c1', 'API'), component('c2', 'Email')] }],
        incidents: [
          {
            id: 'open',
            title: 'Payments are slow',
            state: 'identified',
            impact: 'partial_outage',
            started_at: 10,
            // Not on this page: dropped from the affected list.
            components: ['c1', 'c2', 'hidden'],
            updates: [
              { state: 'identified', message: 'Found it.', created_at: 20, impacts: [{ component: 'c2', impact: 'degraded' }] },
              {
                state: 'investigating',
                message: 'Looking.',
                created_at: 10,
                impacts: [
                  { component: 'c1', impact: 'partial_outage' },
                  { component: 'c2', impact: 'major_outage' },
                ],
              },
            ],
          },
          { id: 'old', title: 'Old', state: 'resolved', started_at: 1, resolved_at: 2, components: ['c1'], updates: [] },
        ],
      }),
      '/s/acme'
    )
    expect(view.active.map((incident) => incident.id)).toEqual(['open'])
    expect(view.active[0]?.latest?.message).toBe('Found it.')
    expect(view.active[0]?.affected).toEqual([
      { name: 'API', impact: 'partial_outage' },
      { name: 'Email', impact: 'degraded' },
    ])
  })

  it('puts maintenance under way first and names its components', () => {
    const view = pageView(
      snapshot({
        groups: [{ id: 'g', name: '', default_open: true, components: [component('c1', 'API')] }],
        maintenance: [
          { title: 'Later', description: '', active: false, starts_at: 500, ends_at: 600, components: ['c1'] },
          { title: 'Now', description: '', active: true, components: ['c1'] },
        ],
      }),
      ''
    )
    expect(view.maintenance.map((window) => [window.title, window.components])).toEqual([
      ['Now', ['API']],
      ['Later', ['API']],
    ])
  })

  it('places the logo below the base', () => {
    const appearance = { ...snapshot().appearance, logo: '/logo?v=5' }
    expect(pageView(snapshot({ appearance }), '/s/acme').logo).toBe('/s/acme/logo?v=5')
    expect(pageView(snapshot(), '').logo).toBeUndefined()
  })
})

describe('worst', () => {
  it('ranks maintenance above operational and below every outage', () => {
    expect(worst(['operational', 'maintenance'])).toBe('maintenance')
    expect(worst(['maintenance', 'degraded'])).toBe('degraded')
    expect(worst([])).toBe('none')
  })
})

describe('formatUptime', () => {
  it('rounds down so a failed check never reads 100 %', () => {
    expect(formatUptime(0.99999, 'en')).toBe('99.99%')
    expect(formatUptime(1, 'en')).toBe('100%')
  })
})
