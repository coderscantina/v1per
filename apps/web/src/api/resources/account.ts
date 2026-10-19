import type { ApiKeyId, ApiKeysCreateArgs, ChangePasswordArgs, Preferences } from '~/generated'

import { api } from '..'

/** The caller's own account: password, second factor, API keys and display preferences. */
export const accountResource = {
  changePassword: (args: ChangePasswordArgs) => api.command('account.changePassword', args),
  preferences: (signal?: AbortSignal) => api.query('account.preferences', {}, { signal }),
  updatePreferences: (preferences: Preferences) => api.command('account.updatePreferences', preferences),
  twoFactor: (signal?: AbortSignal) => api.query('twoFactor.status', {}, { signal }),
  beginTwoFactor: () => api.command('twoFactor.begin', {}),
  confirmTwoFactor: (code: string) => api.command('twoFactor.confirm', { code }),
  disableTwoFactor: () => api.command('twoFactor.disable', {}),
  regenerateRecoveryCodes: () => api.command('twoFactor.regenerateRecoveryCodes', {}),
  apiKeys: (signal?: AbortSignal) => api.query('apiKeys.list', {}, { signal }),
  createApiKey: (args: ApiKeysCreateArgs) => api.command('apiKeys.create', args),
  revokeApiKey: (key: ApiKeyId) => api.command('apiKeys.revoke', { key }),
}
