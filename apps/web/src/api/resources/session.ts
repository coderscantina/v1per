import type { LoginArgs, ReauthenticateArgs, SecondFactorProof } from '~/generated'

import { api } from '..'

/** Sign-in state. `current` never fails with `unauthenticated`; a signed-out caller gets `account: null`. */
export const sessionResource = {
  current: (signal?: AbortSignal) => api.query('auth.session', {}, { signal }),
  login: (args: LoginArgs) => api.command('auth.login', args),
  /** The second step of a login; the challenge travels in a cookie from `login`. */
  verifySecondFactor: (proof: SecondFactorProof) => api.command('auth.verifySecondFactor', proof),
  /** A fresh password check, which lets sensitive commands pass for 10 minutes. */
  reauthenticate: (args: ReauthenticateArgs) => api.command('auth.reauthenticate', args),
  logout: () => api.command('auth.logout', {}),
}
