import type { SlaId, SlasCreateArgs, SlasListArgs, SlasUpdateArgs } from '~/generated'

import { api } from '..'

/** SLAs on a node. `list` and `get` follow the node's `slas:<id>` topic. */
export const slasResource = {
  list: (args: SlasListArgs, signal?: AbortSignal) => api.query('slas.list', args, { signal }),
  get: (sla: SlaId, signal?: AbortSignal) => api.query('slas.get', { sla }, { signal }),
  create: (args: SlasCreateArgs) => api.command('slas.create', args),
  update: (args: SlasUpdateArgs) => api.command('slas.update', args),
  delete: (sla: SlaId) => api.command('slas.delete', { sla }),
}
