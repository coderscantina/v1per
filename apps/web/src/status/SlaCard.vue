<script setup lang="ts">
import { formatLatency, formatUptime } from './format'
import { locale, t, type MessageKey } from './messages'
import { formatAverage, formatMinutes, formatPeriod, formatShare, formatTarget, type ClockView, type SlaView } from './sla'
import SlaHistory from './SlaHistory.vue'
import SlaState from './SlaState.vue'
import { periodKindLabel, slaTone, toneLabel } from './tones'

/**
 * One SLA: the running period's commitments, target against actual, then its closed periods.
 * Grant holders also see each covered monitor.
 */
const props = defineProps<{ sla: SlaView }>()

const period = computed(() => props.sla.current)
const heading = computed(() =>
  period.value ? formatPeriod(period.value.start, props.sla.period, locale.value) : t(periodKindLabel[props.sla.period])
)

const clocks = computed(() => {
  const items: { key: string; label: MessageKey; clock: ClockView }[] = []
  if (period.value?.response) items.push({ key: 'response', label: 'sla.response', clock: period.value.response })
  if (period.value?.resolution) items.push({ key: 'resolution', label: 'sla.resolution', clock: period.value.resolution })
  return items
})

function clockSummary(clock: ClockView): string {
  const parts = [t('sla.clock.counted', { met: clock.met, counted: clock.counted })]
  if (clock.averageMs !== undefined) parts.push(t('sla.clock.average', { duration: formatAverage(clock.averageMs) }))
  return parts.join(' · ')
}

function clockTargets(clock: ClockView): string {
  // Each target keeps its words on one line.
  const targets = clock.targets.map((target) => `${t(toneLabel[target.impact])} ${formatMinutes(target.minutes)}`.replaceAll(' ', '\u00a0'))
  return t('sla.clock.targets', { targets: targets.join(', ') })
}

const showLatency = computed(() => period.value?.monitors.some((monitor) => monitor.latencyMs !== undefined || monitor.latencyState) ?? false)
</script>

<template>
  <article
    :data-sla="sla.name"
    class="rounded-panel border border-border bg-card shadow-soft-sm"
  >
    <header class="flex items-start justify-between gap-3 px-4 py-4">
      <div class="min-w-0">
        <h3 class="font-semibold text-primary">{{ sla.name }}</h3>
        <p class="text-sm text-muted">{{ heading }} · {{ t('sla.zone', { zone: sla.timezone }) }}</p>
      </div>
      <SlaState
        v-if="period"
        :state="period.state"
      />
    </header>

    <ul
      v-if="period"
      class="divide-y divide-divider border-t border-border"
    >
      <li
        v-if="period.availability"
        class="flex flex-col gap-2.5 px-4 py-3.5"
        data-commitment="availability"
      >
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <h4 class="font-medium text-primary">{{ t('sla.availability') }}</h4>
            <p class="text-sm text-muted">{{ t(sla.combine === 'each' ? 'sla.availability.each' : 'sla.availability.anyDown', { target: formatTarget(period.availability.target, locale) }) }}</p>
          </div>
          <div class="flex shrink-0 items-center gap-3">
            <span
              v-if="period.availability.actual !== undefined"
              class="text-base font-semibold text-primary tabular-nums"
            >{{ formatUptime(period.availability.actual / 100, locale) }}</span>
            <SlaState
              :state="period.availability.state"
              :quiet="true"
            />
          </div>
        </div>
        <div
          v-if="period.availability.budgetUsed !== undefined"
          :data-tone="slaTone[period.availability.state]"
          class="flex items-center gap-3 text-sm text-muted"
        >
          <span
            class="h-1.5 min-w-16 flex-1 overflow-hidden rounded-full bg-fill"
            aria-hidden="true"
          >
            <span
              class="block h-full rounded-full bg-(--tone)"
              :style="{ width: `${Math.min(period.availability.budgetUsed, 1) * 100}%` }"
            />
          </span>
          <span class="shrink-0 tabular-nums">{{ t('sla.budget', { share: formatShare(period.availability.budgetUsed, locale) }) }}</span>
        </div>
      </li>

      <li
        v-if="period.latency"
        class="flex items-start justify-between gap-3 px-4 py-3.5"
        data-commitment="latency"
      >
        <div class="min-w-0">
          <h4 class="font-medium text-primary">{{ t('sla.latency') }}</h4>
          <p class="text-sm text-muted">{{ t('sla.latency.target', { target: formatLatency(period.latency.targetMs, locale) }) }}</p>
        </div>
        <div class="flex shrink-0 items-center gap-3">
          <span
            v-if="period.latency.actualMs !== undefined"
            class="text-base font-semibold text-primary tabular-nums"
          >{{ formatLatency(period.latency.actualMs, locale) }}</span>
          <SlaState
            :state="period.latency.state"
            :quiet="true"
          />
        </div>
      </li>

      <li
        v-for="item in clocks"
        :key="item.key"
        class="flex items-start justify-between gap-3 px-4 py-3.5"
        :data-commitment="item.key"
      >
        <div class="min-w-0">
          <h4 class="font-medium text-primary">{{ t(item.label) }}</h4>
          <p class="text-sm text-muted">{{ clockTargets(item.clock) }}</p>
          <p
            v-if="item.clock.counted > 0"
            class="text-sm text-muted"
          >{{ clockSummary(item.clock) }}</p>
        </div>
        <div class="flex shrink-0 items-center gap-3">
          <p
            v-if="item.clock.withinTarget !== undefined"
            class="flex items-baseline gap-1 text-sm text-muted"
          >
            <span class="text-base font-semibold text-primary tabular-nums">{{ formatShare(item.clock.withinTarget, locale) }}</span>
            <span class="max-sm:sr-only">{{ t('sla.withinTarget') }}</span>
          </p>
          <span
            v-else
            class="text-sm text-muted"
          >{{ t('sla.clock.none') }}</span>
          <SlaState
            :state="item.clock.state"
            :quiet="true"
          />
        </div>
      </li>
    </ul>
    <p
      v-else
      class="border-t border-border px-4 py-3.5 text-sm text-muted"
    >{{ t('sla.notStarted') }}</p>

    <div
      v-if="period && period.monitors.length > 0"
      class="border-t border-border px-4 py-3.5"
      data-testid="sla-monitors"
    >
      <table class="w-full text-sm">
        <caption class="pb-1.5 text-left text-xs font-medium text-muted">{{ t('sla.monitors') }}</caption>
        <thead class="sr-only">
          <tr>
            <th scope="col">{{ t('sla.monitor') }}</th>
            <th scope="col">{{ t('sla.availability') }}</th>
            <th
              v-if="showLatency"
              scope="col"
            >{{ t('sla.latency') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="monitor in period.monitors"
            :key="monitor.name"
            :data-sla-monitor="monitor.name"
          >
            <th
              scope="row"
              class="py-1 pr-3 text-left font-medium break-words text-primary"
            >{{ monitor.name }}</th>
            <td class="py-1 pr-3 text-right whitespace-nowrap">
              <span class="inline-flex items-center justify-end gap-2">
                <span class="text-primary tabular-nums">{{ monitor.availability === undefined ? t('sla.state.no_data') : formatUptime(monitor.availability / 100, locale) }}</span>
                <SlaState
                  v-if="monitor.availabilityState"
                  :state="monitor.availabilityState"
                  :compact="true"
                />
              </span>
            </td>
            <td
              v-if="showLatency"
              class="py-1 text-right whitespace-nowrap"
            >
              <span class="inline-flex items-center justify-end gap-2">
                <span class="text-primary tabular-nums">{{ monitor.latencyMs === undefined ? '' : formatLatency(monitor.latencyMs, locale) }}</span>
                <SlaState
                  v-if="monitor.latencyState"
                  :state="monitor.latencyState"
                  :compact="true"
                />
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="flex flex-col gap-1.5 border-t border-border px-4 pt-3.5 pb-3">
      <p class="text-xs font-medium text-muted">{{ t('sla.history') }}</p>
      <SlaHistory
        v-if="sla.history.length > 0"
        :periods="sla.history"
        :kind="sla.period"
        :name="sla.name"
      />
      <p
        v-else
        class="text-sm text-muted"
      >{{ t('sla.noHistory') }}</p>
    </div>
  </article>
</template>
