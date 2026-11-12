<script setup lang="ts">
import type { ResponseRange } from '~/generated'

import { t } from './messages'

/** The page's time frame, as a segmented control. Hidden when the audience has one choice only. */
defineProps<{ options: readonly ResponseRange[] }>()
const range = defineModel<ResponseRange>({ required: true })
</script>

<template>
  <div
    v-if="options.length > 1"
    role="group"
    :aria-label="t('range.label')"
    class="flex rounded-control bg-fill p-[3px]"
    data-testid="range-switch"
  >
    <button
      v-for="item in options"
      :key="item"
      type="button"
      :aria-pressed="item === range"
      :data-range="item"
      class="rounded-lg px-2.5 py-1 text-sm font-medium text-muted tabular-nums transition-colors duration-100 hover:text-primary aria-pressed:bg-card aria-pressed:text-primary aria-pressed:shadow-[0_0_0_1px_var(--border-strong)]"
      @click="range = item"
    >{{ t(`range.short.${item}`) }}</button>
  </div>
</template>
