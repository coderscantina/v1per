<script setup lang="ts">
import { t } from './messages'
import { toneLabel } from './tones'
import type { AffectedView } from './view'

/** The components an incident names, each with its current impact. */
defineProps<{ affected: AffectedView[] }>()
</script>

<template>
  <div
    v-if="affected.length > 0"
    class="flex flex-col gap-1.5"
  >
    <p class="text-xs font-medium text-muted">{{ t('incidents.affected') }}</p>
    <ul class="flex flex-wrap gap-2">
      <li
        v-for="item in affected"
        :key="item.name"
        :data-affected="item.name"
        :data-tone="item.impact ?? 'none'"
        class="inline-flex items-center gap-1.5 rounded-md border border-border px-2 py-1 text-sm"
      >
        <span
          class="size-2 shrink-0 rounded-full bg-(--tone)"
          aria-hidden="true"
        />
        <span class="font-medium text-primary">{{ item.name }}</span>
        <span
          v-if="item.impact"
          class="text-muted"
          :data-impact="item.impact"
        >{{ t(toneLabel[item.impact]) }}</span>
      </li>
    </ul>
  </div>
</template>
