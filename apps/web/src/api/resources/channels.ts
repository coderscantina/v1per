import type {
  ChannelId,
  ChannelsCreateArgs,
  ChannelsTestArgs,
  ChannelsUpdateArgs,
  DeliveriesArgs,
  DeliveryId,
  MonitorDeliveriesArgs,
  NodeId,
} from '~/generated'

import { api } from '..'

/**
 * Notification channels on a node. `list` (the node's own, with settings) and `available` (the
 * node's and its ancestors', for the monitor editor) follow `channels:<id>`, which also carries a
 * `channel.delivery` event whenever a delivery is queued or finishes. Sealed fields come back as
 * `{ set }` only.
 */
export const channelsResource = {
  list: (node: NodeId, signal?: AbortSignal) => api.query('channels.list', { node }, { signal }),
  /** One of the node's channels with its settings, on its node's topic. */
  get: (channel: ChannelId, signal?: AbortSignal) => api.query('channels.get', { channel }, { signal }),
  available: (node: NodeId, signal?: AbortSignal) => api.query('channels.available', { node }, { signal }),
  deliveries: (args: DeliveriesArgs, signal?: AbortSignal) => api.query('channels.deliveries', args, { signal }),
  /** Deliveries about one monitor through every channel whose log the caller may read, newest first. */
  monitorDeliveries: (args: MonitorDeliveriesArgs, signal?: AbortSignal) =>
    api.query('channels.monitorDeliveries', args, { signal }),
  delivery: (delivery: DeliveryId, signal?: AbortSignal) => api.query('channels.delivery', { delivery }, { signal }),
  create: (args: ChannelsCreateArgs) => api.command('channels.create', args),
  update: (args: ChannelsUpdateArgs) => api.command('channels.update', args),
  delete: (channel: ChannelId) => api.command('channels.delete', { channel }),
  /** Queues one test message; read its outcome with `delivery`. */
  test: (args: ChannelsTestArgs) => api.command('channels.test', args),
}
