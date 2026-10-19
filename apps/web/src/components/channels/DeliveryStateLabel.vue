<script setup lang="ts">
import type { DeliveryState } from '~/generated'
import { deliveryStateFill, deliveryStateLabels } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

/** A delivery's state as a coloured dot and its word; the word carries the meaning. */
defineProps<{ state: DeliveryState }>()

const { t } = useI18n()
</script>

<template>
  <span
    class="inline-flex items-center gap-2 text-sm whitespace-nowrap text-foreground"
    :data-delivery="state"
  >
    <span
      class="size-2 shrink-0 rounded-full"
      :class="deliveryStateFill[state]"
      aria-hidden="true"
    />
    {{ t(deliveryStateLabels[state]) }}
  </span>
</template>
