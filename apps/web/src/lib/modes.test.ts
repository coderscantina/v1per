import { describe, expect, it } from 'vitest'

import { modeTarget, monitorTarget } from './modes'

describe('mode switch', () => {
  it('swaps a monitor between its page and its editor', () => {
    const place = { name: 'node-monitor', node: '7', monitor: '21' }
    expect(modeTarget(place, 'configure', 'node-members')).toEqual({ name: 'node-monitor-edit', params: { nodeId: '7', monitorId: '21' } })
    expect(modeTarget({ ...place, name: 'node-monitor-edit' }, 'observe', 'node-members')).toEqual({
      name: 'node-monitor',
      params: { nodeId: '7', monitorId: '21' },
    })
  })

  it('leads any other node page to the dashboard or the last configuration tab', () => {
    const place = { name: 'node-incident', node: '7', monitor: null }
    expect(modeTarget(place, 'observe', 'node-members')).toEqual({ name: 'node', params: { nodeId: '7' } })
    expect(modeTarget(place, 'configure', 'node-members')).toEqual({ name: 'node-members', params: { nodeId: '7' } })
    expect(modeTarget({ name: 'account-security', node: null, monitor: null }, 'observe', 'node-members')).toBeNull()
  })

  it('opens a monitor on its page in configure mode for those who may not change it', () => {
    expect(monitorTarget('7', '21', 'configure', false)).toEqual({ name: 'node-monitor', params: { nodeId: '7', monitorId: '21' } })
  })
})
