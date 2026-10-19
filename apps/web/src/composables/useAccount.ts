import { useQuery } from '@tanstack/vue-query'
import { toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'

/** The caller's second factor: enrolled, required, recovery codes left. */
export function useTwoFactorStatus(enabled: MaybeRefOrGetter<boolean> = true) {
  return useQuery({
    queryKey: queryKeys.twoFactor,
    queryFn: ({ signal }) => accountResource.twoFactor(signal),
    enabled: () => toValue(enabled),
  })
}
