import type {
  NodeId,
  NodesCreateArgs,
  NodesDeleteArgs,
  NodesMoveArgs,
  NodesRenameArgs,
  NodesReorderArgs,
  NodesRequireTwoFactorArgs,
} from '~/generated'

import { api } from '..'

/**
 * The node tree. `tree` follows the caller's `user:<id>` topic (`tree.changed`); `get` follows
 * `node:<id>`, which carries edits of the node and its children.
 */
export const nodesResource = {
  tree: (signal?: AbortSignal) => api.query('nodes.tree', {}, { signal }),
  get: (node: NodeId, signal?: AbortSignal) => api.query('nodes.get', { node }, { signal }),
  create: (args: NodesCreateArgs) => api.command('nodes.create', args),
  rename: (args: NodesRenameArgs) => api.command('nodes.rename', args),
  reorder: (args: NodesReorderArgs) => api.command('nodes.reorder', args),
  move: (args: NodesMoveArgs) => api.command('nodes.move', args),
  delete: (args: NodesDeleteArgs) => api.command('nodes.delete', args),
  requireTwoFactor: (args: NodesRequireTwoFactorArgs) => api.command('nodes.requireTwoFactor', args),
}
