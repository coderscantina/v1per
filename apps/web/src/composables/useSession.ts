import { useQuery } from '@tanstack/vue-query'
import { computed } from 'vue'
import { useRouter } from 'vue-router'

import { closeAllLive } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import { sessionResource } from '~/api/resources/session'
import type { LoginResult, SessionState } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { clearSnapshots } from './useLive'

const signedOut: SessionState = { account: null, enrollment_only: false }

const sessionQuery = {
  queryKey: queryKeys.session,
  queryFn: ({ signal }: { signal: AbortSignal }) => sessionResource.current(signal),
  staleTime: Infinity,
}

/** Loads the session once; the router guard awaits the same query before the first page. */
export const loadSession = () => queryClient.ensureQueryData(sessionQuery)

/** The cached session, for code outside components. */
export const currentSession = (): SessionState =>
  queryClient.getQueryData<SessionState>(queryKeys.session) ?? signedOut

/** Changes fields of the cached session after a command that changed them on the server. */
export function patchSession(patch: Partial<SessionState>): void {
  queryClient.setQueryData<SessionState>(queryKeys.session, (old) => ({ ...(old ?? signedOut), ...patch }))
}

/** The signed-in account, or `null`, and whether the session may only enroll a second factor. */
export function useSession() {
  const query = useQuery(sessionQuery)
  return {
    account: computed(() => query.data.value?.account ?? null),
    enrollmentOnly: computed(() => query.data.value?.enrollment_only ?? false),
  }
}

/** Drops every cached result and closes every live stream, so nothing of the previous account stays. */
export function forgetEverything(): void {
  closeAllLive()
  clearSnapshots()
  void queryClient.cancelQueries()
  queryClient.clear()
}

/** What a login answer leads to: the app, second-factor enrollment, or the code step. */
export type LoginStep = 'signed_in' | 'enroll' | 'challenge'

/**
 * Replaces everything cached with the account that just signed in. A second-factor challenge
 * starts no session yet, so nothing changes until the code is verified.
 */
export function applyLogin(result: LoginResult): LoginStep {
  if (result.status === 'second_factor_required') return 'challenge'
  forgetEverything()
  const enrollment_only = result.status === 'enrollment_required'
  queryClient.setQueryData<SessionState>(queryKeys.session, { account: result.account, enrollment_only })
  return enrollment_only ? 'enroll' : 'signed_in'
}

/** Ends the session server-side, then clears restricted caches and shows the sign-in page. */
export function useSignOut() {
  const router = useRouter()
  return async (): Promise<void> => {
    try {
      await sessionResource.logout()
    } finally {
      forgetEverything()
      queryClient.setQueryData<SessionState>(queryKeys.session, signedOut)
      await router.push({ name: 'login' })
    }
  }
}
