import { describe, expect, it } from 'vitest'

import type { LiveFrame } from '~/generated'

import { onFrame, onStreamError, type LiveState } from './live'

const start: LiveState = { cursor: 'page:1@4', connection: 'connecting', failures: 0 }

describe('onFrame', () => {
  it('goes live on subscribed and resumes from its cursor', () => {
    expect(onFrame(start, { kind: 'subscribed', head: 'page:1@6', cursor: 'page:1@4' })).toEqual({
      state: { cursor: 'page:1@4', connection: 'live', failures: 0 },
      action: 'none',
    })
  })

  it('reads the snapshot again on events and moves the cursor', () => {
    const { state, action } = onFrame(
      { ...start, connection: 'reconnecting', failures: 3 },
      { kind: 'events', cursor: 'page:1@5', events: [{ seq: 5, kind: 'page.status_changed' }] }
    )
    expect(action).toBe('refetch')
    expect(state).toEqual({ cursor: 'page:1@5', connection: 'live', failures: 0 })
  })

  it('reloads when the page is deleted', () => {
    const frame = { kind: 'events', cursor: 'page:1@6', events: [{ seq: 6, kind: 'page.deleted' }] } satisfies LiveFrame
    expect(onFrame(start, frame).action).toBe('reload')
  })

  it('starts over from a fresh snapshot on resync and expired', () => {
    expect(onFrame(start, { kind: 'resync', cursor: 'page:1@9', reason: 'unavailable' })).toEqual({
      state: { ...start, cursor: 'page:1@9' },
      action: 'restart',
    })
    expect(onFrame(start, { kind: 'expired', head: 'page:1@90' }).action).toBe('restart')
  })
})

describe('onStreamError', () => {
  it('reconnects with a doubling delay up to 30 seconds, keeping the cursor', () => {
    let state = start
    const delays: number[] = []
    for (let attempt = 0; attempt < 7; attempt++) {
      const next = onStreamError(state)
      delays.push(next.delay)
      state = next.state
    }
    expect(delays).toEqual([1000, 2000, 4000, 8000, 16_000, 30_000, 30_000])
    expect(state).toEqual({ cursor: 'page:1@4', connection: 'reconnecting', failures: 7 })
  })
})
