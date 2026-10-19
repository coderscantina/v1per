<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import type { ProbeState } from '~/generated'
import { formatLatency } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

import StatusLabel from './StatusLabel.vue'

/**
 * Each probe's view of a monitor after its own retries, and whether it counts in the quorum.
 * Scrolls sideways on a phone.
 */
defineProps<{ states: ProbeState[] }>()

const { t, locale } = useI18n()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 10_000 })
const columns = 5
</script>

<template>
  <div
    class="scroll-fade-x relative w-full overflow-x-auto focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
    role="region"
    :aria-label="t('monitors.detail.probes')"
    tabindex="0"
  >
    <Table
      bare
      :label="t('monitors.detail.probes')"
    >
      <TableHeader>
        <TableRow>
          <TableHead>{{ t('monitors.detail.probe') }}</TableHead>
          <TableHead>{{ t('monitors.columns.status') }}</TableHead>
          <TableHead numeric>{{ t('monitors.columns.latency') }}</TableHead>
          <TableHead>{{ t('monitors.detail.live') }}</TableHead>
          <TableHead>{{ t('monitors.detail.received') }}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableEmptyRow
          v-if="states.length === 0"
          :colspan="columns"
          :label="t('monitors.detail.probesEmpty')"
        />
        <TableRow
          v-for="state in states"
          :key="state.probe"
          :data-probe="state.name"
        >
          <TableCell
            kind="subject"
            class="max-w-0 min-w-44"
          >
            <div class="grid min-w-0 gap-0.5">
              <span class="truncate">{{ state.name }}</span>
              <span
                class="truncate text-xs font-normal text-muted"
                :title="state.message"
              >{{ state.message }}</span>
            </div>
          </TableCell>
          <TableCell>
            <StatusLabel :presence="state.status" />
          </TableCell>
          <TableCell kind="numeric">
            <template v-if="state.latency_ms !== undefined">{{ formatLatency(state.latency_ms, locale) }}</template>
            <TableNone v-else />
          </TableCell>
          <TableCell
            class="whitespace-nowrap"
            :data-live="state.live"
          >
            <span :class="state.live ? 'text-foreground' : 'text-muted'">
              {{ state.live ? t('monitors.detail.liveYes') : t('monitors.detail.liveNo') }}
            </span>
          </TableCell>
          <TableCell
            kind="secondary"
            class="whitespace-nowrap"
          >
            <span :title="dateTime(state.received_at)">{{ relative(state.received_at, now.getTime()) }}</span>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>
