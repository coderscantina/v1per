<script setup lang="ts">
import { t } from './messages'
import type { SlaState } from './sla'
import StatusIcon from './StatusIcon.vue'
import { slaIcon, slaLabel, slaTone } from './tones'

/**
 * An SLA state as icon and word. `quiet` follows the page's rule that healthy is quiet: met and
 * on track show only the check. `compact`, for tight rows, shows only the icon, whose shape
 * differs per state. The word always stays for screen readers.
 */
const props = defineProps<{ state: SlaState; quiet?: boolean; compact?: boolean }>()

const hidden = computed(() => props.compact || (props.quiet && (props.state === 'met' || props.state === 'on_track')))
</script>

<template>
  <span
    :data-tone="slaTone[state]"
    :data-sla-state="state"
    class="inline-flex shrink-0 items-center gap-1.5 text-sm font-medium text-primary"
  >
    <StatusIcon
      :name="slaIcon[state]"
      class="size-4 text-(--tone)"
    />
    <span :class="hidden ? 'sr-only' : ''">{{ t(slaLabel[state]) }}</span>
  </span>
</template>
