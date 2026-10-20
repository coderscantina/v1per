import { useDocumentVisibility, useIntervalFn } from '@vueuse/core'
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, effectScope, onScopeDispose, shallowRef, toValue, watch, type EffectScope, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { instanceResource } from '~/api/resources/instance'
import { monitorsResource } from '~/api/resources/monitors'
import type { MonitorId, NodeId, ProbeId } from '~/generated'
import { HEARTBEATS_PAGE_DEFAULT } from '~/generated/limits'
import { historyRanges, historyWindow, type HistoryRange } from '~/lib/monitor-history'

import { useLiveSnapshot, type LiveStatus } from './useLive'

/** History is not pushed, so it is read again this often while on screen. */
export const HISTORY_REFRESH_MS = 60_000

/** Live states from the furthest behind to current. */
const liveStatuses: readonly LiveStatus[] = ['disconnected', 'resyncing', 'connecting', 'stale', 'current']

const isMonitorEvent = (event: { kind: string }) => event.kind.startsWith('monitor.')

/**
 * A node's monitors, live. Its own on `monitors:<id>`; with `subtree`, those of every node below
 * too, on `monitor-tree:<id>`. Each row names the node it sits on.
 */
export function useNodeMonitors(id: MaybeRefOrGetter<NodeId | null>, subtree: MaybeRefOrGetter<boolean> = false) {
  return useLiveSnapshot({
    key: () => {
      const node = toValue(id)
      return node && toValue(subtree) ? `${node}:subtree` : node
    },
    cache: 'monitors.list',
    load: (_, signal) => monitorsResource.list({ node: toValue(id) ?? '', subtree: toValue(subtree) }, signal),
    affects: isMonitorEvent,
  })
}

/** Monitors on a node and below, for pickers such as covered monitors or incident targets, live. */
export function useMonitorTree(id: MaybeRefOrGetter<NodeId | null>) {
  return useNodeMonitors(id, true)
}

/**
 * Monitors of several nodes, live, such as the group choices on the nodes above. With `subtree`,
 * each node's list includes the monitors of every node below it.
 */
export function useMonitorsOf(nodes: MaybeRefOrGetter<NodeId[]>, subtree = false) {
  type Followed = { scope: EffectScope; live: ReturnType<typeof useNodeMonitors> }
  const followed = shallowRef(new Map<NodeId, Followed>())
  watch(
    () => toValue(nodes),
    (next) => {
      const previous = followed.value
      const current = new Map<NodeId, Followed>()
      for (const node of next) {
        const kept = previous.get(node)
        if (kept) current.set(node, kept)
        else {
          // Detached, so each node's snapshot ends exactly when it leaves the list or this scope ends.
          const scope = effectScope(true)
          const live = scope.run(() => useNodeMonitors(node, subtree))
          if (live) current.set(node, { scope, live })
        }
      }
      for (const [node, entry] of previous) if (!current.has(node)) entry.scope.stop()
      followed.value = current
    },
    { immediate: true }
  )
  onScopeDispose(() => {
    for (const entry of followed.value.values()) entry.scope.stop()
  })
  return computed(() => {
    const entries = toValue(nodes).map((node) => followed.value.get(node)?.live)
    const statuses = entries.map((entry) => entry?.status.value ?? 'connecting')
    return {
      lists: entries.map((entry) => entry?.data.value?.monitors ?? null),
      loading: entries.some((entry) => entry?.loading.value),
      error: entries.find((entry) => entry?.error.value)?.error.value ?? null,
      /** The furthest behind of the lists, so one dropped stream shows. */
      status: liveStatuses.find((status) => statuses.includes(status)) ?? 'current',
    }
  })
}

/**
 * Beats reach the history store about a second after the status event (the storage transfer
 * delay), so history is read again this long after a newer beat shows.
 */
const HISTORY_SETTLE_MS = 2_000

/**
 * One monitor, live on its node's `monitors:<id>` topic. That topic carries every monitor of the
 * node, so only events naming this one read it again; a beat newer than the history read
 * refreshes it. A probe's liveness lapses without an event, so the snapshot is also read again
 * every minute.
 */
export function useMonitorDetail(id: MaybeRefOrGetter<MonitorId | null>) {
  const queryClient = useQueryClient()
  const live = useLiveSnapshot({
    key: () => toValue(id),
    cache: 'monitors.get',
    load: (monitor, signal) => monitorsResource.get(monitor, signal),
    affects: (event) => isMonitorEvent(event) && (event.monitor === undefined || event.monitor === toValue(id)),
  })
  const visibility = useDocumentVisibility()
  useIntervalFn(() => {
    if (visibility.value === 'visible' && live.data.value && !live.loading.value) void live.reload()
  }, HISTORY_REFRESH_MS)
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(
    () => [live.data.value?.monitor.id, live.data.value?.state?.beat_at] as const,
    ([monitor, next], [previousMonitor, previous]) => {
      // The first snapshot counts too: its beat may not have reached the store when history was read.
      if (!monitor || next === undefined || (monitor === previousMonitor && next === previous)) return
      clearTimeout(timer)
      timer = setTimeout(
        () => void queryClient.invalidateQueries({ queryKey: queryKeys.monitorHistory(monitor) }),
        HISTORY_SETTLE_MS
      )
    }
  )
  onScopeDispose(() => clearTimeout(timer))
  return live
}

/** The last 24 hours of a node and everything below it, by hour. Not pushed, so read again every minute. */
export function useScopeTrend(node: MaybeRefOrGetter<NodeId | null>) {
  return useQuery({
    queryKey: computed(() => queryKeys.trend(toValue(node) ?? '')),
    queryFn: ({ signal }) => monitorsResource.trend(toValue(node) ?? '', signal),
    enabled: () => toValue(node) !== null,
    refetchInterval: HISTORY_REFRESH_MS,
    staleTime: HISTORY_REFRESH_MS,
  })
}

/** The last 24 hours of each monitor on a node and below, by hour, for the extended wall. */
export function useMonitorHourly(node: MaybeRefOrGetter<NodeId | null>, enabled: MaybeRefOrGetter<boolean> = true) {
  return useQuery({
    queryKey: computed(() => queryKeys.hourly(toValue(node) ?? '')),
    queryFn: ({ signal }) => monitorsResource.hourly(toValue(node) ?? '', signal),
    enabled: () => toValue(node) !== null && toValue(enabled),
    refetchInterval: HISTORY_REFRESH_MS,
    staleTime: HISTORY_REFRESH_MS,
  })
}

/** `probe`: one probe's own results; `null` for the monitor's status. */
type ProbeFilter = MaybeRefOrGetter<ProbeId | null>

export function useMonitorUptime(id: MaybeRefOrGetter<MonitorId | null>, probe: ProbeFilter = null) {
  return useQuery({
    queryKey: computed(() => queryKeys.uptime(toValue(id) ?? '', toValue(probe) ?? '')),
    queryFn: ({ signal }) =>
      monitorsResource.uptime({ monitor: toValue(id) ?? '', probe: toValue(probe) ?? undefined }, signal),
    enabled: () => toValue(id) !== null,
    refetchInterval: HISTORY_REFRESH_MS,
    staleTime: HISTORY_REFRESH_MS,
  })
}

/** The reader's own zone, so a day is the day they lived through. */
export const localTimezone = () => Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'

/**
 * Response times and beat counts of one time frame. The answer carries the window it was read
 * for, so bars and chart stay aligned with the data between reads.
 */
export function useMonitorSeries(
  id: MaybeRefOrGetter<MonitorId | null>,
  range: MaybeRefOrGetter<HistoryRange>,
  probe: ProbeFilter = null
) {
  return useQuery({
    queryKey: computed(() => queryKeys.latency(toValue(id) ?? '', toValue(range), toValue(probe) ?? '')),
    queryFn: async ({ signal }) => {
      const window = historyWindow(toValue(range), Date.now())
      const { resolution } = historyRanges[window.range]
      const series = await monitorsResource.latency(
        { monitor: toValue(id) ?? '', from: window.from, to: window.to, resolution, probe: toValue(probe) ?? undefined },
        signal
      )
      return { window, ...series }
    },
    enabled: () => toValue(id) !== null,
    refetchInterval: HISTORY_REFRESH_MS,
    staleTime: HISTORY_REFRESH_MS,
    placeholderData: (previous) => previous,
  })
}

/** Newest first, a page at a time; each page names the cursor of the next. */
export function useMonitorHeartbeats(id: MaybeRefOrGetter<MonitorId | null>, probe: ProbeFilter = null) {
  return useInfiniteQuery({
    queryKey: computed(() => queryKeys.heartbeats(toValue(id) ?? '', toValue(probe) ?? '')),
    queryFn: ({ pageParam, signal }: { pageParam: string | undefined; signal: AbortSignal }) =>
      monitorsResource.heartbeats(
        { monitor: toValue(id) ?? '', probe: toValue(probe) ?? undefined, cursor: pageParam, limit: HEARTBEATS_PAGE_DEFAULT },
        signal
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor,
    enabled: () => toValue(id) !== null,
    refetchInterval: HISTORY_REFRESH_MS,
    staleTime: HISTORY_REFRESH_MS,
  })
}

/**
 * Where push URLs point: the instance's public URL, or this page's origin when none is set,
 * which is where the browser reached the server.
 */
export function usePublicOrigin() {
  const info = useQuery({
    queryKey: queryKeys.instanceInfo,
    queryFn: ({ signal }) => instanceResource.info(signal),
  })
  return computed(() => info.data.value?.public_url ?? window.location.origin)
}
