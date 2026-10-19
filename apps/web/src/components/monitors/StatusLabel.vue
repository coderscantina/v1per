<script setup lang="ts">
import { presenceFill, presenceLabels, type Presence } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

/** A monitor's status as a coloured dot and its word; the word carries the meaning. */
defineProps<{ presence: Presence; size?: 'sm' | 'lg' }>()

const { t } = useI18n()
</script>

<template>
  <span
    class="inline-flex items-center gap-2 whitespace-nowrap"
    :class="size === 'lg' ? 'text-xl font-semibold text-primary' : 'text-sm text-foreground'"
    :data-status="presence"
  >
    <span
      class="shrink-0 rounded-full"
      :class="[presenceFill[presence], size === 'lg' ? 'size-3' : 'size-2']"
      aria-hidden="true"
    />
    {{ t(presenceLabels[presence]) }}
  </span>
</template>
