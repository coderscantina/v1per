import type { NodeId, ProbeId, ProbesCreateArgs } from '~/generated'

import { api } from '..'

/**
 * Probes registered on a node. `list` (the node's own, revoked ones included) and `available`
 * (the node's and its ancestors', for the monitor editor) follow `probes:<id>`. Health figures
 * (last contact, lag, buffer) change without events, so read them again while on screen.
 */
export const probesResource = {
  list: (node: NodeId, signal?: AbortSignal) => api.query('probes.list', { node }, { signal }),
  available: (node: NodeId, signal?: AbortSignal) => api.query('probes.available', { node }, { signal }),
  /** The answer carries the agent's token, once. */
  create: (args: ProbesCreateArgs) => api.command('probes.create', args),
  revoke: (probe: ProbeId) => api.command('probes.revoke', { probe }),
}
