<script setup lang="ts">
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import type { Heartbeat, ProbeId } from '~/generated'
import { formatLatency } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

import StatusLabel from './StatusLabel.vue'

/**
 * A monitor's checks, newest first, as they were recorded. With `probes` (names by ID), a column
 * says which probe ran each. Scrolls sideways on a phone.
 */
const props = defineProps<{ heartbeats: Heartbeat[] | null; error?: string | null; probes?: Map<ProbeId, string> | null }>()

const { t, locale } = useI18n()
const { dateTime } = useDateFormat()
const columns = computed(() => (props.probes ? 6 : 5))
</script>

<template>
  <div
    class="scroll-fade-x relative w-full overflow-x-auto focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
    role="region"
    :aria-label="t('monitors.detail.heartbeats')"
    tabindex="0"
  >
    <Table
      bare
      :label="t('monitors.detail.heartbeats')"
    >
      <TableHeader>
        <TableRow>
          <TableHead>{{ t('monitors.columns.status') }}</TableHead>
          <TableHead>{{ t('monitors.detail.time') }}</TableHead>
          <TableHead v-if="probes">{{ t('monitors.detail.probe') }}</TableHead>
          <TableHead numeric>{{ t('monitors.columns.latency') }}</TableHead>
          <TableHead
            numeric
            class="max-sm:hidden"
          >
            {{ t('monitors.detail.httpStatus') }}
          </TableHead>
          <TableHead class="w-full">{{ t('monitors.detail.message') }}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableLoadingRow
          v-if="heartbeats === null && !error"
          :colspan="columns"
          :rows="4"
        />
        <TableEmptyRow
          v-else-if="!heartbeats || heartbeats.length === 0"
          :colspan="columns"
          :label="error ?? t('monitors.detail.heartbeatsEmpty')"
        />
        <TableRow
          v-for="beat in heartbeats ?? []"
          :key="`${beat.time}-${beat.probe ?? ''}`"
        >
          <TableCell>
            <StatusLabel :presence="beat.status" />
          </TableCell>
          <TableCell
            kind="secondary"
            class="whitespace-nowrap tabular-nums"
          >
            {{ dateTime(beat.time) }}
          </TableCell>
          <TableCell
            v-if="probes"
            class="max-w-40 truncate whitespace-nowrap"
          >
            <template v-if="beat.probe">{{ probes.get(beat.probe) ?? beat.probe }}</template>
            <TableNone v-else />
          </TableCell>
          <TableCell kind="numeric">
            <template v-if="beat.latency_ms !== undefined">{{ formatLatency(beat.latency_ms, locale) }}</template>
            <TableNone v-else />
          </TableCell>
          <TableCell
            kind="numeric"
            class="max-sm:hidden"
          >
            <template v-if="beat.http_status !== undefined">{{ beat.http_status }}</template>
            <TableNone v-else />
          </TableCell>
          <TableCell class="max-w-0 min-w-48">
            <span
              class="block truncate"
              :title="beat.message"
            >{{ beat.message }}</span>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>
