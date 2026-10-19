import { useInfiniteQuery, useQuery } from '@tanstack/vue-query'
import { computed, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { channelsResource } from '~/api/resources/channels'
import type { ChannelId, DeliveryId, LiveEvent, MonitorId, NodeId } from '~/generated'

import { useLiveSnapshot } from './useLive'
import { HISTORY_REFRESH_MS } from './useMonitors'

/** Changes of channels themselves; deliveries travel on the same topic. */
const isChannelChange = (event: LiveEvent) => event.kind.startsWith('channel.') && event.kind !== 'channel.delivery'

/** The node's own channels with their settings, live on `channels:<id>`. Needs `channels.manage`. */
export function useNodeChannels(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => channelsResource.list(node, signal),
    affects: isChannelChange,
  })
}

/** One channel with its settings, live on its node's topic. Needs `channels.manage`. */
export function useChannel(id: MaybeRefOrGetter<ChannelId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (channel, signal) => channelsResource.get(channel, signal),
    affects: (event) => isChannelChange(event) && (event.channel === undefined || event.channel === toValue(id)),
  })
}

/**
 * Channels a monitor on the node may use: the node's own and its ancestors', nearest first.
 * Changes on an ancestor reach only that node's topic, so this follows the node's own changes.
 */
export function useAvailableChannels(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => channelsResource.available(node, signal),
    affects: isChannelChange,
  })
}

/**
 * A channel's newest deliveries, live: each `channel.delivery` event for it reads them again.
 * `limit` grows the page (at most 100) without dropping what is shown.
 */
export function useChannelDeliveries(channel: MaybeRefOrGetter<ChannelId | null>, limit: MaybeRefOrGetter<number>) {
  const live = useLiveSnapshot({
    key: () => toValue(channel),
    load: (id, signal) => channelsResource.deliveries({ channel: id, limit: toValue(limit) }, signal),
    affects: (event) => event.kind === 'channel.delivery' && event.channel === toValue(channel),
  })
  watch(
    () => toValue(limit),
    () => void live.reload()
  )
  return live
}

/** How often an unfinished delivery is read again, such as a test send. */
const DELIVERY_POLL_MS = 1_000

/** One delivery, read until it is no longer pending. */
export function useDelivery(id: MaybeRefOrGetter<DeliveryId | null>) {
  return useQuery({
    queryKey: computed(() => queryKeys.delivery(toValue(id) ?? '')),
    queryFn: ({ signal }) => channelsResource.delivery(toValue(id) ?? '', signal),
    enabled: () => toValue(id) !== null,
    refetchInterval: (query) => (query.state.data?.delivery.state === 'pending' ? DELIVERY_POLL_MS : false),
  })
}

/** Alerts a monitor's page shows at a time. */
const MONITOR_DELIVERIES_PAGE = 10

/**
 * A monitor's recent alerts, newest first, a page at a time, through every channel whose log
 * the caller may read. Needs `channels.manage` on the monitor's node. Refreshed with its history.
 */
export function useMonitorDeliveries(monitor: MaybeRefOrGetter<MonitorId | null>, enabled: MaybeRefOrGetter<boolean>) {
  return useInfiniteQuery({
    queryKey: computed(() => queryKeys.monitorDeliveries(toValue(monitor) ?? '')),
    queryFn: ({ pageParam, signal }: { pageParam: string | undefined; signal: AbortSignal }) =>
      channelsResource.monitorDeliveries(
        { monitor: toValue(monitor) ?? '', cursor: pageParam, limit: MONITOR_DELIVERIES_PAGE },
        signal
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor,
    enabled: () => toValue(monitor) !== null && toValue(enabled),
    refetchInterval: HISTORY_REFRESH_MS,
    staleTime: HISTORY_REFRESH_MS,
  })
}
