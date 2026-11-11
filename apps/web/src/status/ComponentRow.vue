<script setup lang="ts">
import DayBars from './DayBars.vue'
import { formatUptime } from './format'
import InternalDetail from './InternalDetail.vue'
import { locale, t } from './messages'
import StatusIcon from './StatusIcon.vue'
import { toneIcon, toneLabel } from './tones'
import { componentBasis, explain } from './uptime'
import UptimeExplain from './UptimeExplain.vue'
import type { ComponentView } from './view'

/**
 * One component: its 90-day uptime, which explains itself when asked, and status now, then 90 daily bars; response times for the
 * team, and for visitors where the page shows them. An operational component shows only the check, the word stays for screen readers.
 */
const props = defineProps<{ component: ComponentView; nested: boolean }>()

const explanation = computed(() => explain(componentBasis(props.component.bars)))
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
      <div class="flex shrink-0 items-center gap-3">
        <UptimeExplain
          v-if="component.uptime !== undefined"
          :explanation="explanation"
          :label="t('uptime.explain', { name: component.name, value: formatUptime(component.uptime, locale) })"
        >
          <span class="flex items-baseline gap-1 text-sm text-muted">
            <span class="text-base font-semibold text-primary tabular-nums">{{ formatUptime(component.uptime, locale) }}</span>
            <span class="max-sm:hidden">{{ t('components.uptime') }}</span>
          </span>
        </UptimeExplain>
        <span
          :data-tone="component.status"
          data-testid="component-status"
          class="inline-flex items-center gap-1.5 text-sm font-medium text-primary"
        >
          <StatusIcon
            :name="toneIcon[component.status]"
            class="size-4 text-(--tone)"
          />
          <span :class="component.status === 'operational' ? 'sr-only' : ''">{{ t(toneLabel[component.status]) }}</span>
        </span>
      </div>
    </div>
    <DayBars
      :bars="component.bars"
      :name="component.name"
    />
    <InternalDetail
      v-if="component.detail && (component.detail.probes.length > 0 || component.detail.latency.length > 0)"
      :detail="component.detail"
      :name="component.name"
    />
  </li>
</template>
