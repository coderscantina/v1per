import type { ApiKeyId, ApiKeysCreateArgs, ChangePasswordArgs } from '~/generated'

import { api } from '..'

/** The caller's own account: password, second factor and API keys. */
export const accountResource = {
  changePassword: (args: ChangePasswordArgs) => api.command('account.changePassword', args),
  twoFactor: (signal?: AbortSignal) => api.query('twoFactor.status', {}, { signal }),
  beginTwoFactor: () => api.command('twoFactor.begin', {}),
  confirmTwoFactor: (code: string) => api.command('twoFactor.confirm', { code }),
  disableTwoFactor: () => api.command('twoFactor.disable', {}),
  regenerateRecoveryCodes: () => api.command('twoFactor.regenerateRecoveryCodes', {}),
  apiKeys: (signal?: AbortSignal) => api.query('apiKeys.list', {}, { signal }),
  createApiKey: (args: ApiKeysCreateArgs) => api.command('apiKeys.create', args),
  revokeApiKey: (key: ApiKeyId) => api.command('apiKeys.revoke', { key }),
}
