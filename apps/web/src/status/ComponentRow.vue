<script setup lang="ts">
import DayBars from './DayBars.vue'
import { formatUptime } from './format'
import InternalDetail from './InternalDetail.vue'
import { locale, t } from './messages'
import StatusIcon from './StatusIcon.vue'
import { toneIcon, toneLabel } from './tones'
import type { ComponentView } from './view'

/** One component: its status now, 90 daily bars and the 90-day uptime; response times for the team. */
defineProps<{ component: ComponentView; nested: boolean }>()
</script>

<template>
  <li
    class="flex flex-col gap-3 px-4 py-4"
    :data-component="component.name"
  >
    <div class="flex items-start justify-between gap-3">
      <div class="min-w-0">
        <component
          :is="nested ? 'h4' : 'h3'"
          class="font-medium text-primary"
        >{{ component.name }}</component>
        <p
          v-if="component.description"
          class="text-sm text-muted"
        >{{ component.description }}</p>
      </div>
      <span
        :data-tone="component.status"
        data-testid="component-status"
        class="inline-flex shrink-0 items-center gap-1.5 text-sm text-foreground"
      >
        <StatusIcon
          :name="toneIcon[component.status]"
          class="size-4 text-(--tone)"
        />
        {{ t(toneLabel[component.status]) }}
      </span>
    </div>
    <DayBars
      :bars="component.bars"
      :name="component.name"
    />
    <div class="flex items-center justify-between gap-2 text-xs text-muted">
      <span>{{ t('components.daysAgo') }}</span>
      <span
        v-if="component.uptime !== undefined"
        class="font-medium text-foreground"
      >{{ t('components.uptime', { value: formatUptime(component.uptime, locale) }) }}</span>
      <span>{{ t('components.today') }}</span>
    </div>
    <InternalDetail
      v-if="component.detail && (component.detail.probes.length > 0 || component.detail.latency.length > 0)"
      :detail="component.detail"
      :name="component.name"
    />
  </li>
</template>
