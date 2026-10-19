import type { IncidentId, IncidentsAddUpdateArgs, IncidentsCreateArgs, IncidentsListArgs, IncidentsUpdateArgs } from '~/generated'

import { api } from '..'

/**
 * Incidents on a node, manual and automatic. `list` and `get` follow the node's `incidents:<id>`
 * topic, which records changes below the node too.
 */
export const incidentsResource = {
  list: (args: IncidentsListArgs, signal?: AbortSignal) => api.query('incidents.list', args, { signal }),
  get: (incident: IncidentId, signal?: AbortSignal) => api.query('incidents.get', { incident }, { signal }),
  create: (args: IncidentsCreateArgs) => api.command('incidents.create', args),
  addUpdate: (args: IncidentsAddUpdateArgs) => api.command('incidents.addUpdate', args),
  update: (args: IncidentsUpdateArgs) => api.command('incidents.update', args),
  delete: (incident: IncidentId) => api.command('incidents.delete', { incident }),
}
