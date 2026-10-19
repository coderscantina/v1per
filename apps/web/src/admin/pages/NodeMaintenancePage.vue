<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { maintenanceResource } from '~/api/resources/maintenance'
import Icon from '~/components/Icon.vue'
import MaintenanceStatusLabel from '~/components/maintenance/MaintenanceStatusLabel.vue'
import WindowList from '~/components/maintenance/WindowList.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useNodeMaintenance } from '~/composables/useMaintenance'
import { localTimezone } from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { Maintenance } from '~/generated'
import { can } from '~/lib/access'
import { strategyLabels } from '~/lib/maintenance-form'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's planned maintenance, live: each window's status (scheduled, active, ended, paused),
 * its strategy, the next window and how many monitors it covers. Members pause, resume, edit
 * and delete. A monitor covered by an active window reports maintenance and sends no alerts.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'maintenance.manage'))
usePageMeta(() => t('maintenance.title'))

const live = useNodeMaintenance(routeId)
const order: Record<Maintenance['status'], number> = { under_maintenance: 0, scheduled: 1, inactive: 2, unknown: 3, ended: 4 }
const rows = computed(() =>
  [...(live.data.value?.maintenances ?? [])].sort((a, b) => order[a.status] - order[b.status] || a.title.localeCompare(b.title))
)
const zone = (maintenance: Maintenance) => maintenance.schedule.timezone ?? localTimezone()

const notice = ref<string | null>(null)
const act = useMutation({
  mutationFn: (action: () => Promise<unknown>) => action(),
  onMutate: () => (notice.value = null),
})
const toggle = (maintenance: Maintenance) =>
  act.mutate(() => (maintenance.active ? maintenanceResource.pause(maintenance.id) : maintenanceResource.resume(maintenance.id)))
const remove = async (maintenance: Maintenance) => {
  const ok = await confirm({
    title: t('maintenance.deleteTitle'),
    message: t('maintenance.deleteConfirm', { title: maintenance.title }),
    confirmLabel: t('maintenance.delete'),
    variant: 'destructive',
  })
  if (ok) {
    act.mutate(() => maintenanceResource.delete(maintenance.id), {
      onSuccess: () => (notice.value = t('maintenance.deleted', { title: maintenance.title })),
    })
  }
}

const loading = computed(() => !live.data.value && !live.error.value)
const columns = 6
</script>

<template>
  <div>
    <PageHeader
      :title="t('maintenance.title')"
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
          <RouterLink :to="{ name: 'node-maintenance-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('maintenance.create') }}
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
        icon="lucide:wrench"
        :title="t('maintenance.empty')"
        :description="manage ? t('maintenance.emptyHint') : t('maintenance.emptyReadOnly')"
      >
        <Button
          v-if="manage && routeId"
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-maintenance-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('maintenance.create') }}
          </RouterLink>
        </Button>
      </EmptyState>

      <Table
        v-else
        :label="t('maintenance.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead class="w-36">{{ t('maintenance.columns.status') }}</TableHead>
            <TableHead>{{ t('maintenance.columns.title') }}</TableHead>
            <TableHead class="max-md:hidden">{{ t('maintenance.columns.strategy') }}</TableHead>
            <TableHead class="max-sm:hidden">{{ t('maintenance.columns.next') }}</TableHead>
            <TableHead
              numeric
              class="max-md:hidden"
            >
              {{ t('maintenance.columns.monitors') }}
            </TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="loading"
            :colspan="columns"
            :rows="3"
          />
          <TableEmptyRow
            v-else-if="live.error.value && !live.data.value"
            :colspan="columns"
            :label="message(live.error.value)"
          />
          <TableRow
            v-for="maintenance in rows"
            :key="maintenance.id"
            :data-maintenance-row="maintenance.title"
          >
            <TableCell :label="t('maintenance.columns.status')">
              <MaintenanceStatusLabel :status="maintenance.status" />
            </TableCell>
            <TableCell
              kind="subject"
              class="max-w-0 min-w-48"
            >
              <div class="grid min-w-0 gap-0.5">
                <RouterLink
                  v-if="manage"
                  :to="{ name: 'node-maintenance-edit', params: { nodeId: maintenance.node, maintenanceId: maintenance.id } }"
                  class="truncate rounded-sm underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                >
                  {{ maintenance.title }}
                </RouterLink>
                <span
                  v-else
                  class="truncate"
                >{{ maintenance.title }}</span>
                <span
                  v-if="maintenance.description"
                  class="truncate text-xs font-normal text-muted"
                >{{ maintenance.description }}</span>
              </div>
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap max-md:hidden"
            >
              {{ t(strategyLabels[maintenance.schedule.strategy]) }}
            </TableCell>
            <TableCell
              class="max-sm:hidden"
              :label="t('maintenance.columns.next')"
            >
              <WindowList
                v-if="maintenance.windows.length > 0 && maintenance.status !== 'ended'"
                :windows="maintenance.windows"
                :timezone="zone(maintenance)"
                :limit="1"
              />
              <TableNone v-else />
            </TableCell>
            <TableCell
              kind="numeric"
              class="max-md:hidden"
            >
              {{ maintenance.monitors.length }}
            </TableCell>
            <TableCell class="text-right whitespace-nowrap">
              <template v-if="manage">
                <Button
                  variant="ghost"
                  size="icon"
                  :disabled="act.isPending.value"
                  :aria-label="
                    maintenance.active
                      ? t('maintenance.pauseNamed', { title: maintenance.title })
                      : t('maintenance.resumeNamed', { title: maintenance.title })
                  "
                  :title="maintenance.active ? t('maintenance.pause') : t('maintenance.resume')"
                  @click="toggle(maintenance)"
                >
                  <Icon :name="maintenance.active ? 'lucide:pause' : 'lucide:play'" />
                </Button>
                <Button
                  as-child
                  variant="ghost"
                  size="icon"
                >
                  <RouterLink
                    :to="{ name: 'node-maintenance-edit', params: { nodeId: maintenance.node, maintenanceId: maintenance.id } }"
                    :aria-label="t('maintenance.editNamed', { title: maintenance.title })"
                  >
                    <Icon name="lucide:pencil" />
                  </RouterLink>
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  :disabled="act.isPending.value"
                  :aria-label="t('maintenance.deleteNamed', { title: maintenance.title })"
                  @click="remove(maintenance)"
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
