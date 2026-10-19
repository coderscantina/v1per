import type { LiveFrame, PageSnapshot } from '~/generated'

import { PageError, type PageClient } from './client'

export interface LiveState {
  /** Where the stream resumes: `page:<id>@<seq>`. */
  cursor: string
  /** Stream failures in a row. */
  failures: number
}

/**
 * What a frame asks of the page. `refetch`: read the snapshot again. `restart`: the stream
 * ended or the cursor is gone, so read the snapshot and subscribe from its cursor. `reload`:
 * the page is gone, reload to show what the server says now.
 */
export type LiveAction = 'none' | 'refetch' | 'restart' | 'reload'

export function onFrame(state: LiveState, frame: LiveFrame): { state: LiveState; action: LiveAction } {
  switch (frame.kind) {
    case 'subscribed':
      return { state: { cursor: frame.cursor, failures: 0 }, action: 'none' }
    case 'events': {
      const next = { cursor: frame.cursor, failures: 0 }
      if (frame.events.some((event) => event.kind === 'page.deleted')) return { state: next, action: 'reload' }
      return { state: next, action: frame.events.length > 0 ? 'refetch' : 'none' }
    }
    case 'resync':
      return { state: { ...state, cursor: frame.cursor }, action: 'restart' }
    case 'expired':
      return { state, action: 'restart' }
  }
}

/** The stream failed: count it and say how long to wait before resuming, 1 s doubling to 30 s. */
export function onStreamError(state: LiveState): { state: LiveState; delay: number } {
  const failures = state.failures + 1
  return {
    state: { ...state, failures },
    delay: Math.min(30_000, 1000 * 2 ** (failures - 1)),
  }
}

export interface FollowOptions {
  client: PageClient
  cursor: string
  apply: (snapshot: PageSnapshot) => void
  reload: () => void
}

/**
 * Follows `<base>/events` from the snapshot's cursor: reads the snapshot again on each event
 * frame, coalescing reads that pile up, resumes from the last cursor after a drop, and starts
 * over from a fresh snapshot on `resync` or `expired`. Returns the stop function.
 */
export function followPage({ client, cursor, apply, reload }: FollowOptions): () => void {
  let state: LiveState = { cursor, failures: 0 }
  let close: (() => void) | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  let reading: Promise<PageSnapshot | null> | null = null
  let again = false

  /** Reads the snapshot; a closed page reloads, a network failure leaves the current one. */
  const read = (): Promise<PageSnapshot | null> => {
    if (reading) {
      again = true
      return reading
    }
    reading = client
      .snapshot()
      .then((snapshot) => {
        if (!stopped) apply(snapshot)
        return snapshot
      })
      .catch((error: unknown) => {
        if (error instanceof PageError && error.code === 'unauthenticated') reload()
        return null
      })
      .finally(() => {
        reading = null
        if (again && !stopped) {
          again = false
          void read()
        }
      })
    return reading
  }

  const open = () => {
    if (stopped) return
    close = client.events(state.cursor, handle, failed)
  }

  const failed = () => {
    close = null
    const { state: next, delay } = onStreamError(state)
    state = next
    timer = setTimeout(() => {
      // After repeated drops, read the snapshot: it shows changes and finds lost access.
      if (state.failures >= 2) void read()
      open()
    }, delay)
  }

  const restart = async () => {
    close?.()
    close = null
    const snapshot = await read()
    if (stopped) return
    if (!snapshot) return failed()
    state = { ...state, cursor: snapshot.cursor }
    open()
  }

  function handle(frame: LiveFrame) {
    const { state: next, action } = onFrame(state, frame)
    state = next
    if (action === 'refetch') void read()
    else if (action === 'restart') void restart()
    else if (action === 'reload') reload()
  }

  open()
  return () => {
    stopped = true
    clearTimeout(timer)
    close?.()
  }
}
