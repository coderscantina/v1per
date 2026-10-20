<script setup lang="ts">
import { formatShortDate, formatUptime } from './format'
import { locale, t, type MessageKey } from './messages'
import type { PageFigures } from './view'

/** The page's headline figures, value above label, so the row reads as numbers first. */
const props = defineProps<{ figures: PageFigures }>()

const items = computed((): { label: MessageKey; value: string }[] => {
  const { uptime, incidents, maintenance } = props.figures
  return [
    { label: 'figures.uptime', value: uptime === undefined ? t('status.none') : formatUptime(uptime, locale.value) },
    { label: 'figures.incidents', value: String(incidents) },
    {
      label: 'figures.maintenance',
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
      :key="item.label"
      class="flex min-w-0 flex-col-reverse gap-0.5 px-3 py-3 sm:px-4 sm:py-4"
    >
      <dt class="text-[13px] leading-4 text-muted">{{ t(item.label) }}</dt>
      <dd class="truncate text-xl font-semibold text-primary tabular-nums sm:text-2xl">{{ item.value }}</dd>
    </div>
  </dl>
</template>
