<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import type { ChannelId, Delivery } from '~/generated'
import { deliveryEventLabels } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

import DeliveryStateLabel from './DeliveryStateLabel.vue'

/**
 * Deliveries newest first: state, event, Kuma's message line, attempts and when. A failed or
 * retrying one shows the provider's error under its message, a pending one when it is tried
 * next. `null` while loading. With `channels`, each row names its channel.
 */
const props = defineProps<{
  deliveries: Delivery[] | null
  label: string
  error?: string | null
  empty: string
  channels?: ReadonlyMap<ChannelId, string>
}>()

const { t } = useI18n()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 10_000 })
const columns = computed(() => (props.channels ? 6 : 5))
</script>

<template>
  <Table :label="label">
    <TableHeader>
      <TableRow>
        <TableHead class="w-32">{{ t('channels.deliveries.columns.state') }}</TableHead>
        <TableHead v-if="channels">{{ t('channels.deliveries.columns.channel') }}</TableHead>
        <TableHead class="max-md:hidden">{{ t('channels.deliveries.columns.event') }}</TableHead>
        <TableHead>{{ t('channels.deliveries.columns.message') }}</TableHead>
        <TableHead
          numeric
          class="max-sm:hidden"
        >
          {{ t('channels.deliveries.columns.attempts') }}
        </TableHead>
        <TableHead>{{ t('channels.deliveries.columns.time') }}</TableHead>
      </TableRow>
    </TableHeader>
    <TableBody>
      <TableLoadingRow
        v-if="deliveries === null && !error"
        :colspan="columns"
        :rows="3"
      />
      <TableEmptyRow
        v-else-if="error"
        :colspan="columns"
        :label="error"
      />
      <TableEmptyRow
        v-else-if="deliveries?.length === 0"
        :colspan="columns"
        :label="empty"
      />
      <TableRow
        v-for="delivery in deliveries ?? []"
        :key="delivery.id"
        :data-delivery-row="delivery.state"
      >
        <TableCell :label="t('channels.deliveries.columns.state')">
          <DeliveryStateLabel :state="delivery.state" />
        </TableCell>
        <TableCell
          v-if="channels"
          kind="secondary"
          :label="t('channels.deliveries.columns.channel')"
        >
          {{ (delivery.channel && channels.get(delivery.channel)) ?? '' }}
        </TableCell>
        <TableCell
          kind="secondary"
          class="whitespace-nowrap max-md:hidden"
        >
          {{ t(deliveryEventLabels[delivery.event]) }}
        </TableCell>
        <TableCell
          kind="subject"
          class="max-w-0 min-w-48"
        >
          <div class="grid min-w-0 gap-0.5">
            <span class="truncate font-normal text-foreground" :title="delivery.summary">{{ delivery.summary }}</span>
            <span
              v-if="delivery.error"
              class="text-xs font-normal break-words whitespace-normal text-destructive select-text"
            >{{ delivery.error }}</span>
            <span
              v-else-if="delivery.detail"
              class="truncate text-xs font-normal text-muted"
              :title="delivery.detail"
            >{{ delivery.detail }}</span>
          </div>
        </TableCell>
        <TableCell
          kind="numeric"
          class="max-sm:hidden"
          :label="t('channels.deliveries.columns.attempts')"
        >
          {{ delivery.attempts }}
        </TableCell>
        <TableCell
          kind="secondary"
          class="whitespace-nowrap"
          :label="t('channels.deliveries.columns.time')"
        >
          <div class="grid gap-0.5">
            <span :title="dateTime(delivery.created_at)">{{ relative(delivery.created_at, now.getTime()) }}</span>
            <span
              v-if="delivery.state === 'pending' && delivery.next_attempt_at"
              class="text-xs"
              :title="dateTime(delivery.next_attempt_at)"
            >{{ t('channels.deliveries.nextAttempt', { time: relative(delivery.next_attempt_at, now.getTime()) }) }}</span>
          </div>
        </TableCell>
      </TableRow>
    </TableBody>
  </Table>
</template>
