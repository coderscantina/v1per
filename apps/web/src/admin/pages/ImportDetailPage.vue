<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ImportStateBadge from '~/components/imports/ImportStateBadge.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useImportInventory, useImportRun, useImports } from '~/composables/useImports'
import { usePageMeta } from '~/composables/usePageMeta'
import type { ImportStep } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * One Kuma import: where it came from, how far each step got and, once complete, how its
 * result compares with the source, table by table. Below, the inventory of what was not
 * mapped, or mapped with a loss, with the reason, a page at a time.
 */
const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { dateTime } = useDateFormat()
const route = useRoute()

const nodeId = computed(() => (typeof route.params.importNode === 'string' ? route.params.importNode : null))
const run = useImportRun(nodeId)
const current = computed(() => run.data.value ?? null)
const manifest = computed(() => current.value?.manifest ?? null)
const summaries = useImports()
const name = computed(() => summaries.data.value?.imports.find((entry) => entry.node === nodeId.value)?.node_name ?? null)
usePageMeta(() => name.value ?? t('imports.title'))

const inventory = useImportInventory(nodeId)
const items = computed(() => (inventory.data.value ? inventory.data.value.pages.flatMap((page) => page.items) : null))

const stepLabels: Record<ImportStep, MessageKey> = {
  control: 'imports.steps.control',
  stats: 'imports.steps.stats',
  heartbeats: 'imports.steps.heartbeats',
  reconcile: 'imports.steps.reconcile',
}

const number = (value: number) => new Intl.NumberFormat(locale.value).format(value)
const unmatched = computed(() => manifest.value?.uptime.per_monitor.filter((monitor) => !monitor.matched) ?? [])
</script>

<template>
  <div>
    <PageHeader
      :title="name ?? t('imports.title')"
      :description="current?.source"
    >
      <template #actions>
        <Button
          as-child
          variant="fill"
        >
          <RouterLink :to="{ name: 'instance-imports' }">
            <Icon name="lucide:arrow-left" />
            {{ t('imports.all') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="run.error.value && !current"
      icon="lucide:import"
      tone="error"
      :title="t('imports.unavailable')"
      :description="message(run.error.value)"
      class="mx-auto max-w-lg"
    />

    <div
      v-else-if="!current"
      class="grid max-w-6xl gap-6"
    >
      <Skeleton class="h-36 w-full rounded-panel" />
      <Skeleton class="h-64 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-6xl gap-6"
    >
      <div class="grid gap-6 lg:grid-cols-2">
        <Panel
          id="import-run"
          :title="t('imports.run')"
        >
          <dl class="grid gap-3 text-sm">
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.columns.state') }}</dt>
              <dd>
                <ImportStateBadge
                  :state="current.state"
                  :reconciled="manifest?.reconciled"
                />
              </dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.columns.node') }}</dt>
              <dd>
                <RouterLink
                  :to="{ name: 'node', params: { nodeId: current.node } }"
                  class="text-primary hover:underline"
                >
                  {{ name ?? current.node }}
                </RouterLink>
              </dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.columns.started') }}</dt>
              <dd class="text-primary">{{ dateTime(current.started_at) }}</dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.columns.finished') }}</dt>
              <dd class="text-primary">
                <template v-if="current.finished_at">{{ dateTime(current.finished_at) }}</template>
                <TableNone v-else />
              </dd>
            </div>
          </dl>
          <p
            v-if="current.state === 'running'"
            class="mt-4 text-sm text-muted"
          >
            {{ t('imports.resumeHint') }}
          </p>
        </Panel>

        <Panel
          id="import-steps"
          :title="t('imports.stepsTitle')"
          flush
        >
          <div class="border-t border-hairline">
            <Table :label="t('imports.stepsTitle')">
              <TableHeader>
                <TableRow>
                  <TableHead>{{ t('imports.columns.step') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.rows') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.skipped') }}</TableHead>
                  <TableHead>{{ t('imports.columns.state') }}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableEmptyRow
                  v-if="current.steps.length === 0"
                  :colspan="4"
                  :label="t('imports.noSteps')"
                />
                <TableRow
                  v-for="step in current.steps"
                  :key="step.step"
                >
                  <TableCell kind="subject">{{ t(stepLabels[step.step]) }}</TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.rows')"
                  >
                    {{ number(step.rows) }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.skipped')"
                  >
                    {{ number(step.skipped) }}
                  </TableCell>
                  <TableCell :label="t('imports.columns.state')">
                    <Badge
                      :variant="step.done ? 'success' : 'warning'"
                      size="sm"
                    >
                      {{ step.done ? t('imports.stepDone') : t('imports.stepOpen') }}
                    </Badge>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
        </Panel>
      </div>

      <template v-if="manifest">
        <Panel
          id="import-source"
          :title="t('imports.reconcile.title')"
        >
          <dl class="grid gap-3 text-sm">
            <div class="grid gap-0.5 sm:grid-cols-[12rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.reconcile.source') }}</dt>
              <dd class="font-mono text-xs break-all text-primary">{{ manifest.source.path }}</dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[12rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.reconcile.sha256') }}</dt>
              <dd class="font-mono text-xs break-all text-primary">{{ manifest.source.sha256 }}</dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[12rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.reconcile.unchanged') }}</dt>
              <dd class="text-primary">
                {{ manifest.source.unchanged ? t('imports.reconcile.unchangedYes') : t('imports.reconcile.unchangedNo') }}
              </dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[12rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.reconcile.nodes') }}</dt>
              <dd class="text-primary tabular-nums">
                {{ t('imports.reconcile.created', { created: number(manifest.nodes.created), expected: number(manifest.nodes.expected) }) }}
              </dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[12rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.reconcile.rawSince') }}</dt>
              <dd class="text-primary">{{ dateTime(manifest.raw_since) }}</dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[12rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('imports.reconcile.uptime') }}</dt>
              <dd class="text-primary tabular-nums">
                {{ t('imports.reconcile.uptimeMatched', { matched: number(manifest.uptime.matched), monitors: number(manifest.uptime.monitors) }) }}
                <span
                  v-if="unmatched.length > 0"
                  class="block text-muted"
                >
                  {{ t('imports.reconcile.uptimeDiffers', { names: unmatched.map((monitor) => monitor.name).join(', ') }) }}
                </span>
              </dd>
            </div>
          </dl>
          <ul
            v-if="manifest.notes.length > 0"
            class="mt-4 grid list-disc gap-1 border-t border-divider pt-4 pl-5 text-sm text-foreground"
            :aria-label="t('imports.reconcile.notes')"
          >
            <li
              v-for="note in manifest.notes"
              :key="note"
            >
              {{ note }}
            </li>
          </ul>
        </Panel>

        <Panel
          id="import-tables"
          :title="t('imports.reconcile.tables')"
          flush
        >
          <div class="border-t border-hairline">
            <Table :label="t('imports.reconcile.tables')">
              <TableHeader>
                <TableRow>
                  <TableHead>{{ t('imports.columns.table') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.inSource') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.mapped') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.inventoried') }}</TableHead>
                  <TableHead>{{ t('imports.columns.result') }}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="table in manifest.tables"
                  :key="table.table"
                >
                  <TableCell
                    kind="subject"
                    class="font-mono text-xs"
                  >
                    {{ table.table }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.inSource')"
                  >
                    {{ number(table.source) }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.mapped')"
                  >
                    {{ number(table.mapped) }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.inventoried')"
                  >
                    {{ number(table.inventoried) }}
                  </TableCell>
                  <TableCell :label="t('imports.columns.result')">
                    <Badge
                      :variant="table.reconciled ? 'success' : 'destructive'"
                      size="sm"
                    >
                      {{ table.reconciled ? t('imports.matches') : t('imports.differs') }}
                    </Badge>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
        </Panel>

        <Panel
          id="import-history"
          :title="t('imports.reconcile.history')"
          flush
        >
          <div class="border-t border-hairline">
            <Table :label="t('imports.reconcile.history')">
              <TableHeader>
                <TableRow>
                  <TableHead>{{ t('imports.columns.table') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.inSource') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.imported') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.skipped') }}</TableHead>
                  <TableHead numeric>{{ t('imports.columns.outsideWindow') }}</TableHead>
                  <TableHead>{{ t('imports.columns.result') }}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="table in manifest.history"
                  :key="table.table"
                >
                  <TableCell
                    kind="subject"
                    class="font-mono text-xs"
                  >
                    {{ table.table }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.inSource')"
                  >
                    {{ number(table.source) }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.imported')"
                  >
                    {{ number(table.imported) }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.skipped')"
                  >
                    {{ number(table.skipped) }}
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    :label="t('imports.columns.outsideWindow')"
                  >
                    {{ number(table.outside_window) }}
                  </TableCell>
                  <TableCell :label="t('imports.columns.result')">
                    <Badge
                      :variant="table.reconciled ? 'success' : 'destructive'"
                      size="sm"
                    >
                      {{ table.reconciled ? t('imports.matches') : t('imports.differs') }}
                    </Badge>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
        </Panel>

        <Panel
          v-if="manifest.logins.length > 0"
          id="import-logins"
          :title="t('imports.logins.title')"
          :meta="t('imports.logins.meta')"
          flush
        >
          <div class="border-t border-hairline">
            <Table :label="t('imports.logins.title')">
              <TableHeader>
                <TableRow>
                  <TableHead>{{ t('imports.logins.kuma') }}</TableHead>
                  <TableHead>{{ t('imports.logins.email') }}</TableHead>
                  <TableHead>{{ t('imports.logins.password') }}</TableHead>
                  <TableHead>{{ t('imports.logins.flags') }}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="login in manifest.logins"
                  :key="login.kuma_username"
                >
                  <TableCell kind="subject">{{ login.kuma_username }}</TableCell>
                  <TableCell :label="t('imports.logins.email')">{{ login.email }}</TableCell>
                  <TableCell
                    kind="secondary"
                    :label="t('imports.logins.password')"
                  >
                    {{ login.password === 'kept' ? t('imports.logins.kept') : t('imports.logins.reset') }}
                  </TableCell>
                  <TableCell :label="t('imports.logins.flags')">
                    <div class="flex flex-wrap gap-1">
                      <Badge
                        v-if="login.instance_admin"
                        variant="secondary"
                        size="sm"
                      >
                        {{ t('imports.logins.admin') }}
                      </Badge>
                      <Badge
                        v-if="login.two_factor"
                        variant="secondary"
                        size="sm"
                      >
                        {{ t('imports.logins.twoFactor') }}
                      </Badge>
                      <TableNone v-if="!login.instance_admin && !login.two_factor" />
                    </div>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
        </Panel>
      </template>

      <Panel
        id="import-inventory"
        :title="t('imports.inventory.title')"
        :meta="t('imports.inventory.meta')"
        flush
      >
        <div class="border-t border-hairline">
          <Table :label="t('imports.inventory.title')">
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('imports.inventory.item') }}</TableHead>
                <TableHead>{{ t('imports.inventory.reason') }}</TableHead>
                <TableHead class="max-md:hidden">{{ t('imports.inventory.source') }}</TableHead>
                <TableHead class="max-md:hidden">{{ t('imports.inventory.columns') }}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableLoadingRow
                v-if="items === null && !inventory.error.value"
                :colspan="4"
                :rows="3"
              />
              <TableEmptyRow
                v-else-if="items === null || items.length === 0"
                :colspan="4"
                :label="inventory.error.value ? message(inventory.error.value) : t('imports.inventory.empty')"
              />
              <TableRow
                v-for="(item, index) in items ?? []"
                :key="`${item.source_table}:${item.source_id}:${index}`"
              >
                <TableCell kind="subject">
                  {{ item.label }}
                  <template #detail>{{ item.kind }}</template>
                </TableCell>
                <TableCell :label="t('imports.inventory.reason')">
                  <span class="whitespace-normal">{{ item.reason }}</span>
                </TableCell>
                <TableCell
                  kind="secondary"
                  :label="t('imports.inventory.source')"
                  class="font-mono text-xs whitespace-nowrap max-md:hidden"
                >
                  {{ item.source_table }} #{{ item.source_id }}
                </TableCell>
                <TableCell
                  kind="secondary"
                  :label="t('imports.inventory.columns')"
                  class="font-mono text-xs max-md:hidden"
                >
                  <template v-if="item.columns.length > 0">{{ item.columns.join(', ') }}</template>
                  <TableNone v-else />
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
        <div
          v-if="inventory.hasNextPage.value"
          class="border-t border-hairline p-3"
        >
          <Button
            variant="fill"
            size="sm"
            :loading="inventory.isFetchingNextPage.value"
            @click="inventory.fetchNextPage()"
          >
            {{ t('imports.inventory.more') }}
          </Button>
        </div>
      </Panel>
    </div>
  </div>
</template>
