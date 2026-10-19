<script setup lang="ts">
import ComponentRow from './ComponentRow.vue'
import { t } from './messages'
import StatusIcon from './StatusIcon.vue'
import { toneIcon, toneLabel } from './tones'
import type { GroupView } from './view'

/** A named group folds open and shut, open at first as the page says; an unnamed one is a plain card. */
defineProps<{ group: GroupView }>()
</script>

<template>
  <details
    v-if="group.name"
    :open="group.defaultOpen"
    :data-group="group.name"
    class="group/details rounded-panel border border-border bg-card shadow-soft-sm"
  >
    <summary class="flex cursor-pointer list-none items-center gap-3 rounded-panel px-4 py-3 [&::-webkit-details-marker]:hidden">
      <StatusIcon
        name="chevron"
        class="size-4 shrink-0 -rotate-90 text-muted transition-transform group-open/details:rotate-0"
      />
      <h3 class="min-w-0 flex-1 truncate font-semibold text-primary">{{ group.name }}</h3>
      <span
        :data-tone="group.status"
        class="inline-flex shrink-0 items-center gap-1.5 text-sm text-foreground"
      >
        <StatusIcon
          :name="toneIcon[group.status]"
          class="size-4 text-(--tone)"
        />
        {{ t(toneLabel[group.status]) }}
      </span>
    </summary>
    <ul class="divide-y divide-divider border-t border-border">
      <ComponentRow
        v-for="component in group.components"
        :key="component.id"
        :component="component"
        :nested="true"
      />
    </ul>
  </details>
  <ul
    v-else
    class="divide-y divide-divider rounded-panel border border-border bg-card shadow-soft-sm"
  >
    <ComponentRow
      v-for="component in group.components"
      :key="component.id"
      :component="component"
      :nested="false"
    />
  </ul>
</template>
