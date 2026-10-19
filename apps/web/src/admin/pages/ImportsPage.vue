<script setup lang="ts">
import ImportStateBadge from '~/components/imports/ImportStateBadge.vue'
import PageHeader from '~/components/PageHeader.vue'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useImports } from '~/composables/useImports'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

/**
 * Every Kuma import on the instance, newest first, with whether it matched its source. Imports
 * run from the command line; this page only reads them.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { dateTime } = useDateFormat()
usePageMeta(() => t('imports.title'))

const imports = useImports()
const rows = computed(() => imports.data.value?.imports ?? [])

const COLUMNS = 5
</script>

<template>
  <div>
    <PageHeader
      :title="t('imports.title')"
      :description="t('imports.description')"
    />

    <EmptyState
      v-if="imports.error.value"
      icon="lucide:import"
      tone="error"
      :title="t('imports.unavailable')"
      :description="message(imports.error.value)"
      class="mx-auto max-w-lg"
    />
    <EmptyState
      v-else-if="imports.data.value && rows.length === 0"
      icon="lucide:import"
      :title="t('imports.empty')"
      :description="t('imports.emptyHint')"
      class="mx-auto max-w-lg"
    />
    <Panel
      v-else
      id="imports-list"
      class="max-w-6xl"
      flush
    >
      <Table :label="t('imports.title')">
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('imports.columns.node') }}</TableHead>
            <TableHead>{{ t('imports.columns.source') }}</TableHead>
            <TableHead>{{ t('imports.columns.state') }}</TableHead>
            <TableHead>{{ t('imports.columns.started') }}</TableHead>
            <TableHead>{{ t('imports.columns.finished') }}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="imports.isPending.value"
            :colspan="COLUMNS"
            :rows="3"
          />
          <TableRow
            v-for="run in rows"
            :key="run.node"
          >
            <TableCell kind="subject">
              <RouterLink
                :to="{ name: 'instance-import', params: { importNode: run.node } }"
                class="hover:underline"
              >
                {{ run.node_name }}
              </RouterLink>
            </TableCell>
            <TableCell
              kind="secondary"
              :label="t('imports.columns.source')"
              class="max-w-xs truncate font-mono text-xs"
              :title="run.source"
            >
              {{ run.source }}
            </TableCell>
            <TableCell :label="t('imports.columns.state')">
              <ImportStateBadge
                :state="run.state"
                :reconciled="run.reconciled"
              />
            </TableCell>
            <TableCell
              kind="secondary"
              :label="t('imports.columns.started')"
              class="whitespace-nowrap"
            >
              {{ dateTime(run.started_at) }}
            </TableCell>
            <TableCell
              kind="secondary"
              :label="t('imports.columns.finished')"
              class="whitespace-nowrap"
            >
              <template v-if="run.finished_at">{{ dateTime(run.finished_at) }}</template>
              <TableNone v-else />
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Panel>
  </div>
</template>
