import type {
  MaintenanceCreateArgs,
  MaintenanceId,
  MaintenanceListArgs,
  MaintenanceSchedule,
  MaintenanceUpdateArgs,
} from '~/generated'

import { api } from '..'

/**
 * Maintenance windows on a node. `list` and `get` follow the node's `maintenance:<id>` topic,
 * which also records `maintenance.status_changed` when a window starts or ends. `preview` reads
 * the next windows of a schedule that is not saved yet.
 */
export const maintenanceResource = {
  list: (args: MaintenanceListArgs, signal?: AbortSignal) => api.query('maintenance.list', args, { signal }),
  get: (maintenance: MaintenanceId, signal?: AbortSignal) => api.query('maintenance.get', { maintenance }, { signal }),
  preview: (schedule: MaintenanceSchedule, signal?: AbortSignal) => api.query('maintenance.preview', schedule, { signal }),
  create: (args: MaintenanceCreateArgs) => api.command('maintenance.create', args),
  update: (args: MaintenanceUpdateArgs) => api.command('maintenance.update', args),
  pause: (maintenance: MaintenanceId) => api.command('maintenance.pause', { maintenance }),
  resume: (maintenance: MaintenanceId) => api.command('maintenance.resume', { maintenance }),
  delete: (maintenance: MaintenanceId) => api.command('maintenance.delete', { maintenance }),
}
