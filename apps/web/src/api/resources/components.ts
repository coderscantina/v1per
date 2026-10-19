import type { ComponentId, ComponentsCreateArgs, ComponentsListArgs, ComponentsUpdateArgs } from '~/generated'

import { api } from '..'

/** Static components: status page entries without a monitor. `list` follows `components:<id>`. */
export const componentsResource = {
  list: (args: ComponentsListArgs, signal?: AbortSignal) => api.query('components.list', args, { signal }),
  create: (args: ComponentsCreateArgs) => api.command('components.create', args),
  update: (args: ComponentsUpdateArgs) => api.command('components.update', args),
  delete: (component: ComponentId) => api.command('components.delete', { component }),
}
