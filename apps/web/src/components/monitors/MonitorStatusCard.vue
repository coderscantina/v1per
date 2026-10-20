<script setup lang="ts">
import type { StatusCardStyle } from '~/generated'

import type { StatusCardProps } from './status/status-facts'
import StatusBanner from './status/StatusBanner.vue'
import StatusCompact from './status/StatusCompact.vue'
import StatusGauge from './status/StatusGauge.vue'
import StatusHero from './status/StatusHero.vue'

/**
 * A monitor's status at a glance in the design the reader picked: how long it has held, the
 * latest message and check, and uptime over Kuma's four windows. The default slot holds controls
 * below, such as a manual monitor's status.
 */
const props = defineProps<StatusCardProps & { design: StatusCardStyle }>()

const designs = { banner: StatusBanner, hero: StatusHero, compact: StatusCompact, gauge: StatusGauge }
</script>

<template>
  <component
    :is="designs[props.design]"
    :presence="presence"
    :state="state"
    :uptime="uptime"
  >
    <template
      v-if="$slots.default"
      #default
    >
      <slot />
    </template>
  </component>
</template>
