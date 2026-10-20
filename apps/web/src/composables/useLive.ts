import { computed, onScopeDispose, ref, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { joinLive, topicOf } from '~/api/live'
import type { LiveEvent, SnapshotMeta, TopicFrame } from '~/generated'

export type LiveStatus = 'connecting' | 'current' | 'stale' | 'resyncing' | 'disconnected'

/**
 * One subscription for the life of the calling scope (architecture patterns, Live snapshot
 * adapter). It opens only once a cursor is known, resumes from the last cursor the server
 * handed out, and asks for a fresh snapshot when the server ends the topic (`expired`, or
 * `resync` because access could not be confirmed). The tab carries every topic on one stream
 * (`joinLive`), which also owns the retry delay.
 */
export function useSubscription(
  topic: MaybeRefOrGetter<string | null>,
  cursor: MaybeRefOrGetter<string | null>,
  handlers: { events: (events: LiveEvent[]) => void; reset: () => void }
) {
  const connected = ref(false)
  const dropped = ref(false)
  const resyncing = ref(false)
  let position: string | null = null
  let leave: (() => void) | null = null

  const stop = () => {
    leave?.()
    leave = null
    connected.value = false
  }

  const open = () => {
    const name = toValue(topic)
    if (!name || !position) return
    // Join before leaving, so moving to a newer cursor keeps a stream this was alone on. A new
    // listener each time, because leaving removes the old one.
    const previous = leave
    leave = joinLive(name, position, {
      frame,
      dropped: () => {
        connected.value = false
        dropped.value = true
      },
    })
    previous?.()
  }

  // The server ending the topic leaves nothing to leave; the fresh snapshot joins it again.
  const frame = (message: TopicFrame) => {
    switch (message.kind) {
      case 'subscribed':
        connected.value = true
        dropped.value = false
        resyncing.value = false
        return
      case 'events':
        position = message.cursor
        handlers.events(message.events)
        return
      case 'resync':
        position = message.cursor
        resyncing.value = true
        connected.value = false
        leave = null
        handlers.reset()
        return
      case 'expired':
        leave = null
        connected.value = false
        position = null
        handlers.reset()
    }
  }

  // A new snapshot cursor (first load, reset) restarts the stream from it.
  watch(
    () => [toValue(topic), toValue(cursor)] as const,
    ([name, next]) => {
      if (!name || !next) {
        stop()
        return
      }
      if (next === position && leave) return
      position = next
      open()
    },
    { immediate: true }
  )
  onScopeDispose(stop)

  return { connected, dropped, resyncing, stop }
}

/** The latest snapshot of each cached kind and key, shown at once when a view opens again. */
const snapshots = new Map<string, SnapshotMeta>()
const SNAPSHOTS_MAX = 200

/** Drops every remembered snapshot, as on sign-out. */
export function clearSnapshots(): void {
  snapshots.clear()
}

/**
 * A live snapshot: reads it, follows the topic its cursor names from that cursor, and reads it again
 * at most once per batch of events that change it. Events at or below the snapshot's revision
 * are already in it. Reads never overlap; events during a read schedule exactly one more. A key
 * change drops the data and starts a new generation, so a late answer for the old key is
 * discarded. The snapshot lives here, not in the query cache: a replay gap is a reset, never a
 * cache miss. With `cache`, the last snapshot of the same kind and key shows at once while the
 * fresh read runs, so going back to a view does not flash a skeleton.
 */
export function useLiveSnapshot<T extends SnapshotMeta>(options: {
  /** What is loaded; a change reloads from scratch. */
  key: MaybeRefOrGetter<string | null>
  load: (key: string, signal: AbortSignal) => Promise<T>
  /** Whether an event changes this snapshot. Every event does by default. */
  affects?: (event: LiveEvent) => boolean
  /** A name for this kind of snapshot, such as `probes.list`, to remember it across views. */
  cache?: string
}) {
  const cacheKey = (key: string | null) => (options.cache && key ? `${options.cache}:${key}` : null)
  // Each entry was stored by a reader with the same `cache` name, so it has this reader's type.
  const remembered = (key: string | null) => {
    const at = cacheKey(key)
    return at ? ((snapshots.get(at) as T | undefined) ?? null) : null
  }
  const remember = (key: string, value: T) => {
    const at = cacheKey(key)
    if (!at) return
    snapshots.delete(at)
    snapshots.set(at, value)
    if (snapshots.size > SNAPSHOTS_MAX) snapshots.delete(snapshots.keys().next().value ?? '')
  }

  const data = shallowRef<T | null>(null)
  const error = shallowRef<unknown>(null)
  const loading = ref(false)
  const stale = ref(false)
  let generation = 0
  let again = false
  let controller: AbortController | null = null

  const read = async () => {
    const current = toValue(options.key)
    if (!current) return
    if (loading.value) {
      again = true
      return
    }
    const mine = generation
    loading.value = true
    controller = new AbortController()
    try {
      const result = await options.load(current, controller.signal)
      if (mine !== generation) return
      data.value = result
      remember(current, result)
      error.value = null
      // Events during the read already scheduled the next one; until it lands the data is stale.
      stale.value = again
    } catch (failure) {
      if (mine !== generation) return
      error.value = failure
    } finally {
      if (mine === generation) {
        loading.value = false
        if (again) {
          again = false
          void read()
        }
      }
    }
  }

  watch(
    () => toValue(options.key),
    () => {
      generation += 1
      controller?.abort()
      loading.value = false
      again = false
      data.value = remembered(toValue(options.key))
      error.value = null
      void read()
    },
    { immediate: true }
  )

  const subscription = useSubscription(
    () => (data.value ? topicOf(data.value.cursor) : null),
    () => data.value?.cursor ?? null,
    {
      events: (events) => {
        const revision = data.value?.revision ?? 0
        const affects = options.affects ?? (() => true)
        if (!events.some((event) => event.seq > revision && affects(event))) return
        stale.value = true
        void read()
      },
      reset: () => void read(),
    }
  )

  /** `stale`: an event changed the snapshot and the new one is on its way. */
  const status = computed<LiveStatus>(() => {
    if (subscription.resyncing.value) return 'resyncing'
    if (subscription.dropped.value) return 'disconnected'
    if (!subscription.connected.value) return 'connecting'
    return stale.value ? 'stale' : 'current'
  })

  onScopeDispose(() => controller?.abort())

  return {
    data,
    error,
    loading,
    status,
    connected: subscription.connected,
    dropped: subscription.dropped,
    reload: read,
  }
}
