import type { ImportInventoryArgs, NodeId } from '~/generated'

import { api } from '..'

/**
 * Kuma imports, one per target node, written by `v1per import kuma`. Read-only and for instance
 * admins only. The inventory lists what an import did not map, or mapped with a loss.
 */
export const importsResource = {
  list: (signal?: AbortSignal) => api.query('imports.list', {}, { signal }),
  get: (node: NodeId, signal?: AbortSignal) => api.query('imports.get', { node }, { signal }),
  inventory: (args: ImportInventoryArgs, signal?: AbortSignal) => api.query('imports.inventory', args, { signal }),
}
