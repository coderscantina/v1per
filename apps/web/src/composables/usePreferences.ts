import { useMutation, useQuery } from '@tanstack/vue-query'
import { watch } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import type { AppAccent } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { useAccentColor } from './useAccentColor'

/**
 * The caller's accent, kept in their preferences so every browser they sign in from shows it.
 * The stored one applies once per sign-in; a pick applies at once and is saved back.
 */
export function useAccentPreference() {
  const { accent, setAccent } = useAccentColor()
  const query = useQuery({ queryKey: queryKeys.preferences, queryFn: ({ signal }) => accountResource.preferences(signal) })
  const mutation = useMutation({
    mutationFn: (next: AppAccent) => accountResource.updatePreferences({ accent: next }),
    onSuccess: (saved) => queryClient.setQueryData(queryKeys.preferences, saved),
  })

  const stop = watch(
    () => query.data.value,
    (preferences) => {
      if (!preferences) return
      if (preferences.accent) setAccent(preferences.accent)
      stop()
    },
    { immediate: true }
  )

  const pick = (next: AppAccent) => {
    setAccent(next)
    mutation.mutate(next)
  }

  return { accent, pick }
}
