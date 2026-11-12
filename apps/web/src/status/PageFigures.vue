<script setup lang="ts">
import type { ResponseRange } from '~/generated'

import { formatShortDate, formatUptime } from './format'
import { locale, t, type MessageKey } from './messages'
import { explain } from './uptime'
import UptimeExplain from './UptimeExplain.vue'
import type { PageFigures } from './view'

/**
 * The page's headline figures over its time frame, value above label, so the row reads as
 * numbers first. The uptime explains itself when asked.
 */
const props = defineProps<{ figures: PageFigures; range: ResponseRange }>()

const explanation = computed(() => explain(props.figures.basis))

const items = computed((): { key: MessageKey; label: string; value: string; explained?: boolean }[] => {
  const { uptime, incidents, maintenance } = props.figures
  const range = t(`range.long.${props.range}`)
  return [
    {
      key: 'figures.uptime',
      label: t('figures.uptime', { range }),
      value: uptime === undefined ? t('status.none') : formatUptime(uptime, locale.value),
      explained: uptime !== undefined,
    },
    { key: 'figures.incidents', label: t('figures.incidents', { range }), value: String(incidents) },
    {
      key: 'figures.maintenance',
      label: t('figures.maintenance'),
      value:
        maintenance === 'now'
          ? t('figures.now')
          : maintenance === undefined
            ? t('figures.none')
            : formatShortDate(maintenance, locale.value),
    },
  ]
})
</script>

<template>
  <dl
    data-testid="page-figures"
    class="grid grid-cols-3 divide-x divide-divider rounded-panel border border-border bg-card shadow-soft-sm"
  >
    <div
      v-for="item in items"
      :key="item.key"
      class="flex min-w-0 flex-col-reverse gap-0.5 px-3 py-3 sm:px-4 sm:py-4"
    >
      <dt class="text-[13px] leading-4 text-muted">{{ item.label }}</dt>
      <dd
        v-if="item.explained"
        class="text-xl font-semibold text-primary tabular-nums sm:text-2xl"
      >
        <UptimeExplain
          :explanation="explanation"
          :label="t('uptime.explainPage', { value: item.value, range: t(`chart.span.${range}`) })"
          align="start"
        >{{ item.value }}</UptimeExplain>
      </dd>
      <dd
        v-else
        class="truncate text-xl font-semibold text-primary tabular-nums sm:text-2xl"
      >{{ item.value }}</dd>
    </div>
  </dl>
</template>
