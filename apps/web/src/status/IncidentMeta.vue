<script setup lang="ts">
import { formatAge, formatDate, formatDuration } from './format'
import { locale, t } from './messages'
import type { IncidentView } from './view'

/**
 * An incident's meta line: the day it started, how long ago, optionally the components it
 * affects, and how long it lasted or has lasted so far. The slot leads the line.
 */
const props = defineProps<{ incident: IncidentView; now: number; components?: boolean }>()

const duration = computed(() => {
  const { startedAt, resolvedAt } = props.incident
  return resolvedAt === undefined
    ? t('incidents.ongoingFor', { duration: formatDuration(props.now - startedAt) })
    : t('incidents.resolvedAfter', { duration: formatDuration(resolvedAt - startedAt) })
})
const names = computed(() => (props.components ? props.incident.affected.map((item) => item.name).join(', ') : ''))
</script>

<template>
  <span class="block text-sm text-muted">
    <slot />
    <time :datetime="new Date(incident.startedAt).toISOString()">{{ formatDate(incident.startedAt, locale) }}</time>
    · {{ formatAge(incident.startedAt, now, locale) }}
    <template v-if="names"> · <span class="text-foreground">{{ names }}</span></template>
    · {{ duration }}
  </span>
</template>
