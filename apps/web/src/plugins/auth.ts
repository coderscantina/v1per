import type { Router } from 'vue-router'

import { api } from '~/api'
import { forgetEverything, patchSession } from '~/composables/useSession'

/**
 * `unauthenticated` means the session ended elsewhere (expired, revoked, signed out in another
 * tab). Drop every restricted result and show the sign-in page once. `invalid_credentials` from
 * `auth.login` is a 401 too, but there is no session to end.
 *
 * `second_factor_enrollment_required` means a 2FA requirement now reaches this account: the
 * session may only enroll, so the app moves to the enrollment page and comes back afterwards.
 */
export function installAuthHandler(router: Router): void {
  api.setAuthHandler(() => {
    forgetEverything()
    const current = router.currentRoute.value
    if (current.meta.guest || current.meta.public) return
    void router.push({ name: 'login', query: { redirect: current.fullPath } })
  })
  api.setEnrollmentHandler(() => {
    patchSession({ enrollment_only: true })
    const current = router.currentRoute.value
    if (current.meta.enrollment || current.meta.public) return
    void router.push({ name: 'enroll', query: { redirect: current.fullPath } })
  })
}
