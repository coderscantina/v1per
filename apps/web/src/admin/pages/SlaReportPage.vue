<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ImpactLabel from '~/components/incidents/ImpactLabel.vue'
import SlaStandingBadge from '~/components/slas/SlaStandingBadge.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useFormats } from '~/composables/usePreferences'
import { useSlaFormat } from '~/composables/useSlaFormat'
import { useSlaPeriod } from '~/composables/useSlas'
import type { SlaClock } from '~/generated'
import { weekStart } from '~/lib/formats'
import { combineLabels, periodLabels, slaWeekdays } from '~/lib/sla-form'
import { settled } from '~/lib/sla-period'
import { useI18n } from '~/plugins/i18n'

/**
 * One SLA period as a document for the client: the terms, each commitment with its target,
 * actual and result, the covered monitors and the incident table. It prints to A4 in light
 * colors; the browser's print to PDF is the export.
 */
const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { formats } = useFormats()
const { date, dateTime } = useDateFormat()
const format = useSlaFormat()
const route = useRoute()
const { routeId } = useCurrentNode()
const node = useNodeDetail(routeId)

const slaId = computed(() => (typeof route.params.slaId === 'string' ? route.params.slaId : null))
const start = computed(() => (typeof route.params.start === 'string' ? route.params.start : null))
const live = useSlaPeriod(slaId, start)
const period = computed(() => live.data.value?.period ?? null)
usePageMeta(() => (period.value ? `${period.value.terms.name} · ${format.title(period.value)}` : t('slas.report.title')))

const commitments = computed(() => (period.value ? format.commitments(period.value) : []))
const hasLatency = computed(() => period.value?.monitors.some((entry) => entry.latency_ms !== undefined) ?? false)
const each = computed(() => period.value?.terms.combine === 'each')

/** Business hours per weekday from the reader's first day of the week, "Mon 09:00 to 17:00". */
const hours = computed(() => {
  const terms = period.value?.terms
  if (!terms?.hours?.length) return []
  const first = weekStart(formats.value)
  const name = new Intl.DateTimeFormat(locale.value, { weekday: 'short', timeZone: 'UTC' })
  return slaWeekdays
    .map((_, index) => (first + index) % 7)
    .flatMap((weekday) => {
      const ranges = (terms.hours ?? [])
        .filter((range) => range.weekday === weekday)
        .sort((a, b) => a.start.localeCompare(b.start))
        .map((range) => t('slas.report.hoursRange', { start: range.start, end: range.end }))
      // 4 January 2026 is a Sunday, weekday 0.
      return ranges.length > 0 ? [`${name.format(Date.UTC(2026, 0, 4 + weekday))} ${ranges.join(', ')}`] : []
    })
})
const holidays = computed(() => (period.value?.terms.holidays ?? []).map((day) => date(Date.parse(`${day}T00:00:00Z`), 'UTC')))

const clockText = (clock: SlaClock) => (clock.elapsed_ms === undefined ? '' : format.duration(clock.elapsed_ms))
const printedAt = Date.now()
const print = () => window.print()
</script>

<template>
  <div class="mx-auto grid w-full max-w-[210mm] gap-4">
    <div class="flex items-center gap-2 print:hidden">
      <Button
        as-child
        variant="ghost"
      >
        <RouterLink :to="{ name: 'node-sla', params: { nodeId: routeId, slaId }, query: { period: start } }">
          <Icon name="lucide:arrow-left" />
          {{ t('slas.report.back') }}
        </RouterLink>
      </Button>
      <Button
        v-if="period"
        variant="accent"
        class="ml-auto"
        @click="print"
      >
        <Icon name="lucide:printer" />
        {{ t('slas.report.print') }}
      </Button>
    </div>

    <EmptyState
      v-if="live.error.value && !period"
      icon="lucide:file-check"
      tone="error"
      :title="t('slas.unavailable')"
      :description="message(live.error.value)"
    />

    <Skeleton
      v-else-if="!period"
      class="h-[297mm] w-full rounded-panel"
    />

    <article
      v-else
      class="grid gap-8 rounded-panel border border-hairline bg-card px-[14mm] py-[12mm] text-sm text-foreground shadow-soft-sm [print-color-adjust:exact] max-sm:px-5 max-sm:py-6 print:rounded-none print:border-0 print:p-0 print:shadow-none"
      data-sla-report
    >
      <header class="grid gap-1 border-b border-hairline pb-5">
        <p class="text-[0.8125rem] text-muted">
          {{ t('slas.report.title') }}<template v-if="node.data.value?.node"> · {{ node.data.value.node.name }}</template>
        </p>
        <h1 class="text-2xl font-semibold tracking-tight text-primary">{{ period.terms.name }}</h1>
        <p class="text-base text-primary">{{ format.title(period) }}</p>
        <p class="text-muted">
          {{ format.range(period) }} ·
          {{ period.frozen_at === undefined ? t('slas.view.liveHint') : t('slas.view.frozenAt', { time: dateTime(period.frozen_at) }) }}
        </p>
      </header>

      <section class="grid gap-3 break-inside-avoid">
        <h2 class="text-base font-semibold text-primary">{{ t('slas.report.terms') }}</h2>
        <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5">
          <dt class="text-muted">{{ t('slas.columns.period') }}</dt>
          <dd>{{ t(periodLabels[period.terms.period]) }}</dd>
          <dt class="text-muted">{{ t('slas.editor.startDate') }}</dt>
          <dd>{{ date(Date.parse(`${period.terms.start_date}T00:00:00Z`), 'UTC') }}</dd>
          <dt class="text-muted">{{ t('slas.report.timezone') }}</dt>
          <dd>{{ period.terms.timezone }}</dd>
          <dt class="text-muted">{{ t('slas.report.combine') }}</dt>
          <dd>{{ t(combineLabels[period.terms.combine]) }}</dd>
          <dt class="text-muted">{{ t('slas.report.hours') }}</dt>
          <dd>
            <template v-if="hours.length > 0">
              <span
                v-for="line in hours"
                :key="line"
                class="block"
              >{{ line }}</span>
            </template>
            <template v-else>{{ t('slas.report.aroundTheClock') }}</template>
          </dd>
          <template v-if="holidays.length > 0">
            <dt class="text-muted">{{ t('slas.report.holidays') }}</dt>
            <dd>{{ holidays.join(', ') }}</dd>
          </template>
        </dl>
      </section>

      <section class="grid gap-3 break-inside-avoid">
        <h2 class="text-base font-semibold text-primary">{{ t('slas.view.commitments') }}</h2>
        <table
          v-if="commitments.length > 0"
          class="w-full text-left"
        >
          <thead class="border-b border-hairline text-[0.8125rem] text-muted">
            <tr>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.report.commitment') }}</th>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.report.target') }}</th>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.report.actual') }}</th>
              <th class="py-1.5 text-right font-normal">{{ t('slas.report.result') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="entry in commitments"
              :key="entry.key"
              class="border-b border-hairline align-top last:border-0"
              :data-commitment="entry.key"
            >
              <td class="py-2 pr-3 font-medium text-primary">{{ entry.label }}</td>
              <td class="py-2 pr-3">{{ entry.target }}</td>
              <td class="py-2 pr-3">
                <span class="font-semibold text-primary tabular-nums">{{ entry.actual ?? t('slas.view.noData') }}</span>
                <span
                  v-for="note in entry.notes"
                  :key="note"
                  class="block text-muted"
                >{{ note }}</span>
                <span
                  v-if="entry.budget !== undefined"
                  class="block text-muted"
                >{{ t('slas.view.budgetUsed', { value: format.share(entry.budget) }) }}</span>
              </td>
              <td class="py-2 text-right">
                <SlaStandingBadge :standing="entry.outcome" />
              </td>
            </tr>
          </tbody>
        </table>
        <p
          v-else
          class="text-muted"
        >
          {{ t('slas.view.noTargets') }}
        </p>
      </section>

      <section class="grid gap-3">
        <h2 class="text-base font-semibold text-primary">{{ t('slas.view.monitors') }}</h2>
        <table class="w-full text-left">
          <thead class="border-b border-hairline text-[0.8125rem] text-muted">
            <tr>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.view.monitor') }}</th>
              <th class="py-1.5 pr-3 text-right font-normal">{{ t('slas.view.uptime') }}</th>
              <th
                v-if="each"
                class="py-1.5 pr-3 text-right font-normal"
              >
                {{ t('slas.view.budget') }}
              </th>
              <th
                v-if="hasLatency"
                class="py-1.5 text-right font-normal"
              >
                {{ t('slas.view.avgLatency') }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="entry in period.monitors"
              :key="entry.monitor"
              class="border-b border-hairline break-inside-avoid last:border-0"
            >
              <td class="py-2 pr-3 text-primary">{{ entry.name }}</td>
              <td class="py-2 pr-3 text-right tabular-nums">
                <span class="inline-flex items-center gap-2">
                  {{ entry.availability === undefined ? t('slas.view.noData') : format.percent(entry.availability) }}
                  <SlaStandingBadge
                    v-if="settled(entry.availability_outcome)"
                    :standing="entry.availability_outcome"
                  />
                </span>
              </td>
              <td
                v-if="each"
                class="py-2 pr-3 text-right tabular-nums"
              >
                {{ entry.budget_used === undefined ? t('slas.view.noData') : format.share(entry.budget_used) }}
              </td>
              <td
                v-if="hasLatency"
                class="py-2 text-right tabular-nums"
              >
                <span class="inline-flex items-center gap-2">
                  {{ entry.latency_ms === undefined ? t('slas.view.noData') : format.ms(entry.latency_ms) }}
                  <SlaStandingBadge
                    v-if="settled(entry.latency_outcome)"
                    :standing="entry.latency_outcome"
                  />
                </span>
              </td>
            </tr>
          </tbody>
        </table>
      </section>

      <section class="grid gap-3">
        <h2 class="text-base font-semibold text-primary">{{ t('slas.view.incidents') }}</h2>
        <table
          v-if="period.incidents.length > 0"
          class="w-full text-left"
        >
          <thead class="border-b border-hairline text-[0.8125rem] text-muted">
            <tr>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.view.incident') }}</th>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.view.impact') }}</th>
              <th class="py-1.5 pr-3 font-normal">{{ t('slas.view.response') }}</th>
              <th class="py-1.5 font-normal">{{ t('slas.view.resolution') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="entry in period.incidents"
              :key="entry.incident"
              class="border-b border-hairline align-top break-inside-avoid last:border-0"
            >
              <td class="py-2 pr-3">
                <span class="block text-primary">{{ entry.title }}</span>
                <span class="block text-[0.8125rem] text-muted">{{ dateTime(entry.started_at, period.terms.timezone) }}</span>
              </td>
              <td class="py-2 pr-3">
                <ImpactLabel :impact="entry.impact" />
              </td>
              <td
                v-for="clock in [entry.response, entry.resolution]"
                :key="clock === entry.response ? 'response' : 'resolution'"
                class="py-2 pr-3 last:pr-0"
              >
                <span class="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <SlaStandingBadge :standing="clock.outcome" />
                  <span class="text-[0.8125rem] text-muted tabular-nums">{{ clockText(clock) }}</span>
                </span>
              </td>
            </tr>
          </tbody>
        </table>
        <p
          v-else
          class="text-muted"
        >
          {{ t('slas.view.noIncidents') }}
        </p>
      </section>

      <footer class="border-t border-hairline pt-4 text-[0.8125rem] text-muted">
        {{ t('slas.report.printed', { time: dateTime(printedAt) }) }}
      </footer>
    </article>
  </div>
</template>

<style>
@page {
  size: A4;
  margin: 14mm;
}
</style>
