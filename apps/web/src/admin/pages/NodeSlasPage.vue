<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { slasResource } from '~/api/resources/slas'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useFormats } from '~/composables/usePreferences'
import { useNodeSlas } from '~/composables/useSlas'
import type { Sla } from '~/generated'
import { can } from '~/lib/access'
import { formatDate, formatNumber } from '~/lib/formats'
import { impactLabels } from '~/lib/incidents'
import { combineLabels, durationParts, periodLabels, slaImpacts } from '~/lib/sla-form'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's SLAs, live: each one's period, how availability combines over its monitors and
 * its targets. Staff with Admin above the node add, edit and delete them; a client reads them.
 */
const { t, locale } = useI18n()
const { formats, numberLocale } = useFormats()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'slas.manage'))
usePageMeta(() => t('slas.title'))

const live = useNodeSlas(routeId)

const duration = (total: number) => {
  const { hours, minutes } = durationParts(total)
  return [hours > 0 ? t('slas.units.hours', { count: hours }) : null, minutes > 0 ? t('slas.units.minutes', { count: minutes }) : null]
    .filter((part) => part !== null)
    .join(' ')
}

/** The targets as short lines: availability and latency first, then times per impact. */
const targets = (sla: Sla): string[] => {
  const lines: string[] = []
  const measures = [
    sla.availability === undefined
      ? null
      : t('slas.targets.availability', { value: formatNumber(numberLocale.value, sla.availability, { maximumFractionDigits: 3 }) }),
    sla.latency_ms === undefined ? null : t('slas.targets.latency', { value: formatNumber(numberLocale.value, sla.latency_ms) }),
  ].filter((part) => part !== null)
  if (measures.length > 0) lines.push(measures.join(' · '))
  for (const impact of slaImpacts) {
    const target = sla.times?.[impact]
    const parts = [
      target?.response_minutes === undefined ? null : t('slas.targets.response', { time: duration(target.response_minutes) }),
      target?.resolution_minutes === undefined ? null : t('slas.targets.resolution', { time: duration(target.resolution_minutes) }),
    ].filter((part) => part !== null)
    if (parts.length > 0) lines.push(`${t(impactLabels[impact])}: ${parts.join(', ')}`)
  }
  return lines
}

const rows = computed(() =>
  [...(live.data.value?.slas ?? [])]
    .sort((a, b) => a.name.localeCompare(b.name, locale.value))
    .map((sla) => ({ sla, targets: targets(sla) }))
)

const since = (sla: Sla) =>
  t('slas.since', {
    date: formatDate(formats.value, Date.parse(`${sla.start_date}T00:00:00Z`), 'UTC'),
    zone: sla.timezone,
  })

const notice = ref<string | null>(null)
const act = useMutation({
  mutationFn: (action: () => Promise<unknown>) => action(),
  onMutate: () => (notice.value = null),
})
const remove = async (sla: Sla) => {
  const name = sla.name
  const ok = await confirm({
    title: t('slas.deleteTitle'),
    message: t('slas.deleteConfirm', { name }),
    confirmLabel: t('slas.delete'),
    variant: 'destructive',
  })
  if (ok) {
    act.mutate(() => slasResource.delete(sla.id), {
      onSuccess: () => (notice.value = t('slas.deleted', { name })),
    })
  }
}

const loading = computed(() => !live.data.value && !live.error.value)
const columns = 6
</script>

<template>
  <div>
    <PageHeader
      :title="t('slas.title')"
      :description="node?.name"
    >
      <template
        v-if="manage && routeId"
        #actions
      >
        <Button
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-sla-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('slas.create') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-6xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-6xl gap-4"
    >
      <div
        aria-live="polite"
        class="empty:hidden"
      >
        <Alert
          v-if="act.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(act.error.value) }}</p>
        </Alert>
        <Alert
          v-else-if="notice"
          color="success"
        >
          <p class="text-sm">{{ notice }}</p>
        </Alert>
      </div>

      <EmptyState
        v-if="live.data.value && rows.length === 0"
        icon="lucide:file-check"
        :title="t('slas.empty')"
        :description="manage ? t('slas.emptyHint') : t('slas.emptyReadOnly')"
      >
        <Button
          v-if="manage && routeId"
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-sla-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('slas.create') }}
          </RouterLink>
        </Button>
      </EmptyState>

      <Table
        v-else
        :label="t('slas.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('slas.columns.name') }}</TableHead>
            <TableHead class="max-md:hidden">{{ t('slas.columns.period') }}</TableHead>
            <TableHead class="max-md:hidden">{{ t('slas.columns.combine') }}</TableHead>
            <TableHead class="max-sm:hidden">{{ t('slas.columns.targets') }}</TableHead>
            <TableHead
              numeric
              class="max-lg:hidden"
            >
              {{ t('slas.columns.monitors') }}
            </TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="loading"
            :colspan="columns"
            :rows="2"
          />
          <TableEmptyRow
            v-else-if="live.error.value && !live.data.value"
            :colspan="columns"
            :label="message(live.error.value)"
          />
          <TableRow
            v-for="{ sla, targets: lines } in rows"
            :key="sla.id"
            :data-sla-row="sla.name"
          >
            <TableCell
              kind="subject"
              class="max-w-0 min-w-48"
            >
              <div class="grid min-w-0 gap-0.5">
                <RouterLink
                  v-if="manage"
                  :to="{ name: 'node-sla-edit', params: { nodeId: sla.node, slaId: sla.id } }"
                  class="truncate rounded-sm underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                >
                  {{ sla.name }}
                </RouterLink>
                <span
                  v-else
                  class="truncate"
                >{{ sla.name }}</span>
                <span class="truncate text-xs font-normal text-muted">{{ since(sla) }}</span>
                <ul
                  v-if="lines.length > 0"
                  class="mt-1 grid gap-0.5 text-sm font-normal text-foreground select-text sm:hidden"
                >
                  <li
                    v-for="line in lines"
                    :key="line"
                  >
                    {{ line }}
                  </li>
                </ul>
              </div>
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap max-md:hidden"
            >
              {{ t(periodLabels[sla.period]) }}
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap max-md:hidden"
            >
              {{ t(combineLabels[sla.combine]) }}
            </TableCell>
            <TableCell
              :label="t('slas.columns.targets')"
              class="max-sm:hidden"
            >
              <ul
                v-if="lines.length > 0"
                class="grid gap-0.5 select-text"
              >
                <li
                  v-for="line in lines"
                  :key="line"
                >
                  {{ line }}
                </li>
              </ul>
              <TableNone v-else />
            </TableCell>
            <TableCell
              kind="numeric"
              class="max-lg:hidden"
            >
              {{ sla.monitors.length }}
            </TableCell>
            <TableCell class="text-right whitespace-nowrap">
              <template v-if="manage">
                <Button
                  as-child
                  variant="ghost"
                  size="icon"
                >
                  <RouterLink
                    :to="{ name: 'node-sla-edit', params: { nodeId: sla.node, slaId: sla.id } }"
                    :aria-label="t('slas.editNamed', { name: sla.name })"
                  >
                    <Icon name="lucide:pencil" />
                  </RouterLink>
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  :disabled="act.isPending.value"
                  :aria-label="t('slas.deleteNamed', { name: sla.name })"
                  @click="remove(sla)"
                >
                  <Icon name="lucide:trash-2" />
                </Button>
              </template>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  </div>
</template>
