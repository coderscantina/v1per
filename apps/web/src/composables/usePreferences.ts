import { useMutation, useQuery } from '@tanstack/vue-query'
import { computed, watch } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import type { AppAccent, Preferences, StatusCardStyle } from '~/generated'
import { numberLocale, type Formats } from '~/lib/formats'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

import { useAccentColor } from './useAccentColor'
import { useSession } from './useSession'

/**
 * The caller's stored preferences and a save that changes some of them. The server replaces the
 * whole set, so a change is merged into what was read; the saved set updates the cache at once.
 */
export function usePreferences() {
  const { account } = useSession()
  const query = useQuery({
    queryKey: queryKeys.preferences,
    queryFn: ({ signal }) => accountResource.preferences(signal),
    enabled: () => account.value !== null,
  })
  const mutation = useMutation({
    mutationFn: (change: Preferences) => accountResource.updatePreferences({ ...query.data.value, ...change }),
    onMutate: (change) => {
      const current = queryClient.getQueryData<Preferences>(queryKeys.preferences)
      queryClient.setQueryData<Preferences>(queryKeys.preferences, { ...current, ...change })
    },
    onSuccess: (saved) => queryClient.setQueryData(queryKeys.preferences, saved),
  })
  return { query, save: (change: Preferences) => mutation.mutate(change) }
}

/**
 * The caller's accent, kept in their preferences so every browser they sign in from shows it.
 * The stored one applies once per sign-in; a pick applies at once and is saved back.
 */
export function useAccentPreference() {
  const { accent, setAccent } = useAccentColor()
  const { query, save } = usePreferences()

  // A flag, not the watcher's stop handle: with cached preferences the first call runs before
  // `watch` returns.
  let applied = false
  watch(
    () => query.data.value,
    (preferences) => {
      if (applied || !preferences) return
      applied = true
      if (preferences.accent) setAccent(preferences.accent)
    },
    { immediate: true }
  )

  const pick = (next: AppAccent) => {
    setAccent(next)
    save({ accent: next })
  }

  return { accent, pick }
}

/** The status block design of the monitor page, banner until the caller picks another. */
export function useStatusCardPreference() {
  const { query, save } = usePreferences()
  return computed({
    get: (): StatusCardStyle => query.data.value?.status_card ?? 'banner',
    set: (status_card: StatusCardStyle) => save({ status_card }),
  })
}

/**
 * How the reader wants numbers, dates and times written, from their preferences and the app
 * language. Signed out, everything follows the language.
 */
export function useFormats() {
  const { locale } = useI18n()
  const { query } = usePreferences()
  const formats = computed(
    (): Formats => ({
      language: locale.value,
      number: query.data.value?.number_format,
      date: query.data.value?.date_format,
      time: query.data.value?.time_format,
      zone: query.data.value?.time_zone,
      week: query.data.value?.week_start,
    })
  )
  return { formats, numberLocale: computed(() => numberLocale(formats.value)) }
}
