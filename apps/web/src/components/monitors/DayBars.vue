<script setup lang="ts">
import type { DayBar } from '~/generated'
import { barFill, barTone, formatUptime } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

/**
 * One bar per day, oldest first, coloured by that day's uptime, with the day and its uptime on
 * hover and an equivalent table for assistive technology.
 */
const props = defineProps<{ bars: DayBar[]; timezone: string }>()

const { t, locale } = useI18n()

const dayText = (day: string) =>
  new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeZone: 'UTC' }).format(Date.parse(`${day}T00:00:00Z`))

const uptimeText = (bar: DayBar) => formatUptime(bar.uptime, locale.value) ?? t('monitors.detail.noData')

const title = (bar: DayBar) => {
  const day = dayText(bar.day)
  if (bar.uptime === undefined && bar.maintenance === 0) return t('monitors.detail.barEmpty', { day })
  const uptime = uptimeText(bar)
  return bar.utc_approximate ? t('monitors.detail.barUtc', { day, uptime }) : t('monitors.detail.barTitle', { day, uptime })
}

const summary = computed(() => {
  const counted = props.bars.filter((bar) => bar.uptime !== undefined)
  const down = counted.filter((bar) => (bar.uptime ?? 1) < 1).length
  return t('monitors.detail.barsSummary', { down, days: props.bars.length })
})
</script>

<template>
  <figure class="m-0 grid gap-2">
    <figcaption class="sr-only">{{ summary }}</figcaption>
    <div
      class="flex h-10 items-stretch gap-px sm:gap-0.5"
      aria-hidden="true"
    >
      <span
        v-for="bar in bars"
        :key="bar.day"
        class="min-w-0 flex-1 rounded-[2px] transition-opacity hover:opacity-70"
        :class="barFill[barTone(bar)]"
        :title="title(bar)"
        :data-tone="barTone(bar)"
      />
    </div>
    <div
      class="flex justify-between text-xs text-muted"
      aria-hidden="true"
    >
      <span>{{ t('monitors.detail.barsAgo') }}</span>
      <span>{{ t('monitors.detail.barsZone', { zone: timezone }) }}</span>
      <span>{{ t('monitors.detail.barsToday') }}</span>
    </div>
    <div class="sr-only">
      <table>
        <caption>{{ t('monitors.detail.barsZone', { zone: timezone }) }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t('monitors.detail.day') }}</th>
            <th scope="col">{{ t('monitors.detail.uptime') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="bar in bars"
            :key="bar.day"
          >
            <th scope="row">{{ dayText(bar.day) }}</th>
            <td>{{ uptimeText(bar) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </figure>
</template>
