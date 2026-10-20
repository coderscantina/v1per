<script setup lang="ts">
import { formatDuration, formatTime } from './format'
import { locale, t } from './messages'
import StatusIcon from './StatusIcon.vue'
import type { MaintenanceView } from './view'

/** A maintenance under way or coming up, with a meta line of its window, length and components. */
const props = defineProps<{ maintenance: MaintenanceView }>()

const period = computed(() => {
  const { startsAt, endsAt } = props.maintenance
  const start = startsAt === undefined ? null : formatTime(startsAt, locale.value)
  const end = endsAt === undefined ? null : formatTime(endsAt, locale.value)
  if (start && end) return t('maintenance.window', { start, end })
  if (start) return t('maintenance.from', { start })
  if (end) return t('maintenance.until', { end })
  return null
})

const meta = computed(() => {
  const { startsAt, endsAt, components } = props.maintenance
  const length = startsAt !== undefined && endsAt !== undefined ? t('maintenance.lasts', { duration: formatDuration(endsAt - startsAt) }) : null
  return [period.value, length, components.join(', ')].filter(Boolean).join(' · ')
})
</script>

<template>
  <article
    data-tone="maintenance"
    :data-maintenance="maintenance.title"
    class="flex flex-col gap-1 rounded-panel border border-border bg-card p-4 shadow-soft-sm"
  >
    <header class="flex flex-wrap items-start justify-between gap-x-3 gap-y-1">
      <h3 class="min-w-0 flex-1 font-semibold text-primary">{{ maintenance.title }}</h3>
      <span class="inline-flex shrink-0 items-center gap-1.5 text-sm text-foreground">
        <StatusIcon
          :name="maintenance.active ? 'wrench' : 'calendar'"
          class="size-4 text-(--tone)"
        />
        {{ maintenance.active ? t('maintenance.active') : t('maintenance.scheduled') }}
      </span>
    </header>
    <p
      v-if="meta"
      class="text-sm text-muted"
    >{{ meta }}</p>
    <p
      v-if="maintenance.description"
      class="mt-2 whitespace-pre-line text-foreground"
    >{{ maintenance.description }}</p>
  </article>
</template>
