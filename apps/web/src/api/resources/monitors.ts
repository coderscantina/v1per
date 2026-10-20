import type {
  DayBarsArgs,
  HeartbeatsArgs,
  LatencyArgs,
  MonitorId,
  MonitorsCreateArgs,
  MonitorsListArgs,
  MonitorsMoveArgs,
  MonitorsSetManualStatusArgs,
  MonitorsUpdateArgs,
  NodeId,
  UptimeArgs,
} from '~/generated'

import { api } from '..'

/**
 * Monitors on a node. `list` and `get` follow the node's `monitors:<id>` topic, which carries
 * every change and status change of its monitors; a `subtree` list follows `monitor-tree:<id>`,
 * which carries those of every node below too. History (`heartbeats`, `uptime`, `latency`,
 * `dayBars`, and `trend` and `hourly` for a node and everything below it) is not pushed; read it again when
 * the status changes. With `probe`, history reads
 * that probe's own results instead of the monitor's status.
 */
export const monitorsResource = {
  list: (args: MonitorsListArgs, signal?: AbortSignal) => api.query('monitors.list', args, { signal }),
  get: (monitor: MonitorId, signal?: AbortSignal) => api.query('monitors.get', { monitor }, { signal }),
  heartbeats: (args: HeartbeatsArgs, signal?: AbortSignal) => api.query('monitors.heartbeats', args, { signal }),
  uptime: (args: UptimeArgs, signal?: AbortSignal) => api.query('monitors.uptime', args, { signal }),
  latency: (args: LatencyArgs, signal?: AbortSignal) => api.query('monitors.latency', args, { signal }),
  dayBars: (args: DayBarsArgs, signal?: AbortSignal) => api.query('monitors.dayBars', args, { signal }),
  trend: (node: NodeId, signal?: AbortSignal) => api.query('monitors.trend', { node }, { signal }),
  hourly: (node: NodeId, signal?: AbortSignal) => api.query('monitors.hourly', { node }, { signal }),
  create: (args: MonitorsCreateArgs) => api.command('monitors.create', args),
  update: (args: MonitorsUpdateArgs) => api.command('monitors.update', args),
  pause: (monitor: MonitorId) => api.command('monitors.pause', { monitor }),
  resume: (monitor: MonitorId) => api.command('monitors.resume', { monitor }),
  setManualStatus: (args: MonitorsSetManualStatusArgs) => api.command('monitors.setManualStatus', args),
  delete: (monitor: MonitorId) => api.command('monitors.delete', { monitor }),
  move: (args: MonitorsMoveArgs) => api.command('monitors.move', args),
}
