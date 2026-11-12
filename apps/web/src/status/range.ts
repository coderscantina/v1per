import type { ResponseRange } from '~/generated'

import type { MaybeRefOrGetter } from 'vue'

/**
 * The page's one time frame: bars, uptime figures, the incident count and every response time
 * chart follow it. The audience may pick up to the snapshot's `range_max`; the choice lives in
 * the address as `?range=`, so a shared link opens on the same frame.
 */

export const ranges: readonly ResponseRange[] = ['24h', '7d', '30d', '90d']

const DAY_MS = 86_400_000

export const rangeDays: Record<ResponseRange, number> = { '24h': 1, '7d': 7, '30d': 30, '90d': 90 }

/** How far back a range reaches, in milliseconds. */
export const rangeSpan = (range: ResponseRange) => rangeDays[range] * DAY_MS

/** The ranges up to and including `max`. */
export const rangesUpTo = (max: ResponseRange) => ranges.slice(0, ranges.indexOf(max) + 1)

const PARAM = 'range'

function asked(options: readonly ResponseRange[]): ResponseRange | undefined {
  const value = new URLSearchParams(window.location.search).get(PARAM)
  return options.find((range) => range === value)
}

/** The range the address asks for when the audience may pick it, else the longest it may. */
export function usePageRange(max: MaybeRefOrGetter<ResponseRange>) {
  const options = computed(() => rangesUpTo(toValue(max)))
  const range = ref(asked(options.value) ?? toValue(max))

  // A new snapshot may narrow the choice, after an admin changed the page's setting.
  watch(options, (allowed) => {
    if (!allowed.includes(range.value)) range.value = toValue(max)
  })

  function pick(next: ResponseRange) {
    range.value = next
    const url = new URL(window.location.href)
    if (next === toValue(max)) url.searchParams.delete(PARAM)
    else url.searchParams.set(PARAM, next)
    window.history.replaceState(window.history.state, '', url)
  }

  return { range: readonly(range), options, pick }
}
