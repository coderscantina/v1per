<script setup lang="ts">
import type { ProbeStatus } from '~/generated'
import { probeStatusFill, probeStatusLabels } from '~/lib/probes'
import { useI18n } from '~/plugins/i18n'

/** A probe's connection status as a coloured dot and its word; the word carries the meaning. */
defineProps<{ status: ProbeStatus }>()

const { t } = useI18n()
</script>

<template>
  <span
    class="inline-flex items-center gap-2 text-sm whitespace-nowrap text-foreground"
    :data-probe-status="status"
  >
    <span
      class="size-2 shrink-0 rounded-full"
      :class="probeStatusFill[status]"
      aria-hidden="true"
    />
    {{ t(probeStatusLabels[status]) }}
  </span>
</template>
