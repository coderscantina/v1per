<script setup lang="ts">
import { formatTime } from './format'
import { locale, t } from './messages'
import StatusIcon from './StatusIcon.vue'
import type { MaintenanceView } from './view'

/** A maintenance under way or coming up, with its window and the components it covers. */
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
</script>

<template>
  <article
    data-tone="maintenance"
    :data-maintenance="maintenance.title"
    class="flex flex-col gap-2 rounded-panel border border-border bg-card p-4 shadow-soft-sm"
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
      v-if="period"
      class="text-sm text-muted"
    >{{ period }}</p>
    <p
      v-if="maintenance.description"
      class="whitespace-pre-line text-foreground"
    >{{ maintenance.description }}</p>
    <ul
      v-if="maintenance.components.length > 0"
      class="flex flex-wrap gap-2"
    >
      <li
        v-for="name in maintenance.components"
        :key="name"
        class="rounded-md border border-border px-2 py-1 text-sm font-medium text-primary"
      >{{ name }}</li>
    </ul>
  </article>
</template>
