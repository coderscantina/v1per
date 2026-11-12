<script setup lang="ts">
import type { ResponseRange } from '~/generated'

import { formatUptime } from './format'
import { locale, t } from './messages'
import MonitorDetail from './MonitorDetail.vue'
import StatusIcon from './StatusIcon.vue'
import { toneIcon, toneLabel } from './tones'
import { componentBasis, explain, uptimeOf } from './uptime'
import UptimeBars from './UptimeBars.vue'
import UptimeExplain from './UptimeExplain.vue'
import { barsIn, type ComponentView } from './view'

/**
 * One component over the page's time frame: its uptime, which explains itself when asked, and
 * status now, then its bars; response times for the team, and for visitors where the page shows
 * them. An operational component shows only the check, the word stays for screen readers.
 */
const props = defineProps<{ component: ComponentView; nested: boolean; range: ResponseRange }>()

const bars = computed(() => barsIn(props.component, props.range))
const basis = computed(() => componentBasis(bars.value))
const uptime = computed(() => uptimeOf(basis.value.counts))
const explanation = computed(() => explain(basis.value))
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
          v-if="uptime !== undefined"
          :explanation="explanation"
          :label="t('uptime.explain', { name: component.name, value: formatUptime(uptime, locale), range: t(`chart.span.${range}`) })"
        >
          <span class="flex items-baseline gap-1 text-sm text-muted">
            <span class="text-base font-semibold text-primary tabular-nums">{{ formatUptime(uptime, locale) }}</span>
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
    <UptimeBars
      :bars="bars"
      :name="component.name"
      :range="range"
    />
    <MonitorDetail
      v-if="component.detail && (component.detail.probes.length > 0 || component.detail.latency.length > 0)"
      :detail="component.detail"
      :component="component.id"
      :name="component.name"
      :range="range"
    />
  </li>
</template>
