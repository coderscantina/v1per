import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick } from 'vue'

import { api } from '~/api'
import { closeAllLive, REOPEN_DELAY_MS } from '~/api/live'
import type { LiveFrame } from '~/generated'

import { useLiveSnapshot } from './useLive'

type Snapshot = { revision: number; cursor: string; name: string }

/** Streams the test drives through `emit`, and a `load` whose answers it releases one by one. */
function setup() {
  const sinks: Array<(text: string) => void> = []
  const subscribe = vi.spyOn(api, 'subscribe').mockImplementation((_params, data) => {
    sinks.push(data)
    return () => {}
  })
  const pending: Array<(snapshot: Snapshot) => void> = []
  const load = vi.fn(() => new Promise<Snapshot>((resolve) => pending.push(resolve)))
  const scope = effectScope()
  const live = scope.run(() => useLiveSnapshot({ key: 'instance', load }))!
  const settle = async () => {
    for (let i = 0; i < 5; i++) await nextTick()
    vi.advanceTimersByTime(REOPEN_DELAY_MS)
  }
  return {
    live,
    load,
    subscribe,
    scope,
    settle,
    emit: (frame: LiveFrame) => sinks.at(-1)?.(JSON.stringify({ topic: 'instance', ...frame })),
    answer: (revision: number) => pending.shift()?.({ revision, cursor: `instance@${revision}`, name: `r${revision}` }),
  }
}

describe('useLiveSnapshot', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    closeAllLive()
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('subscribes from the snapshot cursor and skips events already in it', async () => {
    const { live, load, subscribe, emit, answer, settle, scope } = setup()
    answer(3)
    await settle()

    expect(subscribe.mock.calls[0]?.[0]).toEqual(['instance@3'])
    emit({ kind: 'subscribed', head: 'instance@3', cursor: 'instance@3' })
    expect(live.status.value).toBe('current')

    emit({ kind: 'events', cursor: 'instance@3', events: [{ seq: 2, kind: 'x' }, { seq: 3, kind: 'x' }] })
    expect(load).toHaveBeenCalledTimes(1)
    scope.stop()
  })

  it('reads once per batch and never overlaps reads', async () => {
    const { live, load, emit, answer, settle, scope } = setup()
    answer(1)
    await settle()
    emit({ kind: 'subscribed', head: 'instance@1', cursor: 'instance@1' })

    emit({ kind: 'events', cursor: 'instance@3', events: [{ seq: 2, kind: 'x' }, { seq: 3, kind: 'x' }] })
    emit({ kind: 'events', cursor: 'instance@4', events: [{ seq: 4, kind: 'x' }] })
    emit({ kind: 'events', cursor: 'instance@5', events: [{ seq: 5, kind: 'x' }] })
    expect(load).toHaveBeenCalledTimes(2)
    expect(live.status.value).toBe('stale')

    answer(3)
    await settle()
    expect(load).toHaveBeenCalledTimes(3)
    answer(5)
    await settle()
    expect(live.data.value?.name).toBe('r5')
    expect(live.status.value).toBe('current')
    scope.stop()
  })

  it('reads a fresh snapshot when the cursor expired', async () => {
    const { live, load, emit, answer, settle, scope } = setup()
    answer(9)
    await settle()
    emit({ kind: 'expired', head: 'instance@2' })

    expect(load).toHaveBeenCalledTimes(2)
    answer(2)
    await settle()
    expect(live.data.value?.cursor).toBe('instance@2')
    scope.stop()
  })
})
