<script setup lang="ts">
import type { MaintenanceWindow } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Maintenance windows as date ranges in `timezone`, the zone the schedule is written in, so
 * "02:00" reads as 02:00 whatever the reader's own zone.
 */
const props = defineProps<{ windows: readonly MaintenanceWindow[]; timezone: string; limit?: number }>()

const { t, locale } = useI18n()
const format = computed(() => {
  try {
    return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short', timeZone: props.timezone })
  } catch {
    return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' })
  }
})
const shown = computed(() => (props.limit ? props.windows.slice(0, props.limit) : props.windows))

/** One window in words; either end may be open for a single maintenance without dates. */
const describe = (window: MaintenanceWindow): string => {
  const { start, end } = window
  if (start !== undefined && end !== undefined) return format.value.formatRange(start, end)
  if (start !== undefined) return t('maintenance.windows.from', { time: format.value.format(start) })
  if (end !== undefined) return t('maintenance.windows.until', { time: format.value.format(end) })
  return t('maintenance.windows.open')
}
</script>

<template>
  <ol class="grid gap-1.5">
    <li
      v-for="(window, index) in shown"
      :key="`${window.start ?? ''}-${window.end ?? ''}-${index}`"
      class="text-sm text-foreground tabular-nums"
    >
      {{ describe(window) }}
    </li>
  </ol>
</template>
