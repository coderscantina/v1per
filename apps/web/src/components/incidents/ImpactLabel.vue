<script setup lang="ts">
import type { Impact } from '~/generated'
import { impactFill, impactLabels } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/** An impact as a coloured dot and its word; the word carries the meaning. */
defineProps<{ impact: Impact }>()

const { t } = useI18n()
</script>

<template>
  <span
    class="inline-flex items-center gap-2 text-sm whitespace-nowrap text-foreground"
    :data-impact="impact"
  >
    <span
      class="size-2 shrink-0 rounded-full"
      :class="impactFill[impact]"
      aria-hidden="true"
    />
    {{ t(impactLabels[impact]) }}
  </span>
</template>
