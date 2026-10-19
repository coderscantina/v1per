import type { InstanceTestSmtpArgs, InstanceUpdateSettingsArgs, InstanceUpdateSmtpArgs } from '~/generated'

import { api } from '..'

/** This installation: its name, version, public URL and mail server. Follows the `instance` topic. */
export const instanceResource = {
  info: (signal?: AbortSignal) => api.query('instance.info', {}, { signal }),
  /** Instance admins only. */
  settings: (signal?: AbortSignal) => api.query('instance.settings', {}, { signal }),
  updateSettings: (args: InstanceUpdateSettingsArgs) => api.command('instance.updateSettings', args),
  /** Without `smtp`, removes the mail server. */
  updateSmtp: (args: InstanceUpdateSmtpArgs) => api.command('instance.updateSmtp', args),
  /** Queues one test email, through `smtp` as given or the saved server; read its outcome with `channels.delivery`. */
  testSmtp: (args: InstanceTestSmtpArgs) => api.command('instance.testSmtp', args),
}
