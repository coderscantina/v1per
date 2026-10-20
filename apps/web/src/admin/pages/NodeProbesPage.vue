<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'
import { useNow } from '@vueuse/core'

import { probesResource } from '~/api/resources/probes'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import CreateProbeDialog from '~/components/probes/CreateProbeDialog.vue'
import EditProbeDialog from '~/components/probes/EditProbeDialog.vue'
import ProbePlace from '~/components/probes/ProbePlace.vue'
import ProbeStatusLabel from '~/components/probes/ProbeStatusLabel.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useAvailableProbes, useNodeProbes } from '~/composables/useProbes'
import { useFormats } from '~/composables/usePreferences'
import type { Probe } from '~/generated'
import { formatNumber } from '~/lib/formats'
import { can } from '~/lib/access'
import { probeLag, type ProbeLag } from '~/lib/probes'
import { useI18n } from '~/plugins/i18n'

/**
 * The probes a node's monitors can use: its own, with their health and lag, and those of the
 * nodes above, which are managed there. Admins add a probe (its token shows once) and revoke one.
 */
const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime, relative } = useDateFormat()
const now = useNow({ interval: 10_000 })
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'probes.manage'))
usePageMeta(() => t('probes.title'))

const own = useNodeProbes(routeId)
const available = useAvailableProbes(routeId)
const probes = computed(() => own.data.value?.probes ?? [])
const inherited = computed(() => (available.data.value?.probes ?? []).filter((probe) => !probe.own))

const creating = ref(false)
const editing = ref(false)
const edited = ref<Probe | null>(null)
const startEdit = (probe: Probe) => {
  edited.value = probe
  editing.value = true
}
const notice = ref<string | null>(null)
const revoke = useMutation({
  mutationFn: (probe: Probe) => probesResource.revoke(probe.id),
  onMutate: () => (notice.value = null),
  onSuccess: ({ probe }) => (notice.value = t('probes.revoked', { name: probe.name })),
})

const ask = async (probe: Probe) => {
  const ok = await confirm({
    title: t('probes.revokeTitle'),
    message: t('probes.revokeConfirm', { name: probe.name }),
    confirmLabel: t('probes.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(probe)
}

const number = (value: number) => formatNumber(numberLocale.value, value)
const lagText = (lag: ProbeLag) => {
  switch (lag.kind) {
    case 'dropped':
      return t('probes.dropped', { count: number(lag.count) })
    case 'buffered':
      return t('probes.buffered', { count: number(lag.count) })
    case 'late':
      return t('probes.late', { late: number(lag.late), started: number(lag.started) })
  }
}
const lastSeen = (probe: Probe) => {
  if (probe.last_seen_at) return relative(probe.last_seen_at, now.value.getTime())
  return probe.builtin ? t('probes.builtinHint') : t('probes.neverSeen')
}
const skew = (probe: Probe) => Math.round(Math.abs(probe.clock_skew_ms ?? 0) / 1000)

const loading = computed(() => !own.data.value && !own.error.value)
const empty = computed(() => own.data.value !== null && probes.value.length === 0)
const columns = 7
</script>

<template>
  <div>
    <PageHeader
      :title="t('probes.title')"
      :description="node?.name"
    >
      <template
        v-if="manage && routeId"
        #actions
      >
        <Button
          variant="accent"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('probes.create') }}
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
      class="grid max-w-6xl gap-8"
    >
      <Alert
        v-if="node && !manage"
        color="info"
      >
        <p class="text-sm">{{ t('probes.readOnly') }}</p>
      </Alert>
      <div
        aria-live="polite"
        class="empty:hidden"
      >
        <Alert
          v-if="revoke.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(revoke.error.value) }}</p>
        </Alert>
        <Alert
          v-else-if="notice"
          color="success"
        >
          <p class="text-sm">{{ notice }}</p>
        </Alert>
      </div>

      <EmptyState
        v-if="empty"
        icon="lucide:radio-tower"
        :title="t('probes.empty')"
        :description="manage ? t('probes.emptyHint') : t('probes.emptyReadOnly')"
      >
        <Button
          v-if="manage"
          variant="accent"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('probes.create') }}
        </Button>
      </EmptyState>

      <section
        v-else
        class="grid gap-3"
        aria-labelledby="probes-own-title"
      >
        <h2
          id="probes-own-title"
          class="flex items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
        >
          {{ t('probes.own') }}
          <span
            v-if="own.data.value"
            class="text-sm font-normal text-muted tabular-nums"
          >· {{ probes.length }}</span>
        </h2>
        <Table :label="t('probes.own')">
          <TableHeader>
            <TableRow>
              <TableHead class="sm:w-32">{{ t('probes.columns.status') }}</TableHead>
              <TableHead>{{ t('probes.columns.name') }}</TableHead>
              <TableHead class="max-md:hidden">{{ t('probes.columns.version') }}</TableHead>
              <TableHead class="max-sm:hidden">{{ t('probes.columns.lastSeen') }}</TableHead>
              <TableHead class="max-sm:hidden">{{ t('probes.columns.lag') }}</TableHead>
              <TableHead
                numeric
                class="max-sm:hidden"
              >
                {{ t('probes.columns.monitors') }}
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
              v-else-if="own.error.value"
              :colspan="columns"
              :label="message(own.error.value)"
            />
            <TableRow
              v-for="probe in probes"
              :key="probe.id"
              :data-probe="probe.name"
            >
              <TableCell :label="t('probes.columns.status')">
                <ProbeStatusLabel :status="probe.status" />
              </TableCell>
              <TableCell
                kind="subject"
                class="max-w-0 max-sm:w-full sm:min-w-44"
              >
                <div class="grid min-w-0 gap-1">
                  <span
                    class="truncate"
                    :class="probe.revoked_at ? 'text-muted' : undefined"
                  >{{ probe.name }}</span>
                  <ProbePlace
                    :location="probe.location"
                    class="text-sm font-normal text-muted"
                  />
                  <span
                    class="truncate text-xs font-normal text-muted sm:hidden"
                    :title="probe.last_seen_at ? dateTime(probe.last_seen_at) : undefined"
                  >{{ lastSeen(probe) }}</span>
                  <div
                    v-if="probe.clock_skewed || probe.revoked_at"
                    class="flex flex-wrap gap-1"
                  >
                    <Badge
                      v-if="probe.clock_skewed && !probe.revoked_at"
                      variant="warning"
                      size="sm"
                      :title="t('probes.skewedHint')"
                    >
                      {{ t('probes.skewed', { seconds: skew(probe) }) }}
                    </Badge>
                    <Badge
                      v-if="probe.revoked_at"
                      variant="destructive"
                      size="sm"
                    >
                      {{ t('probes.revokedOn', { date: date(probe.revoked_at) }) }}
                    </Badge>
                  </div>
                </div>
              </TableCell>
              <TableCell
                kind="secondary"
                class="whitespace-nowrap max-md:hidden"
              >
                <div
                  v-if="probe.version"
                  class="grid gap-0.5"
                >
                  <span>{{ probe.version }}</span>
                  <span
                    v-if="probe.platform"
                    class="text-muted"
                  >{{ probe.platform }}</span>
                </div>
                <TableNone v-else />
              </TableCell>
              <TableCell
                kind="secondary"
                class="whitespace-nowrap max-sm:hidden"
                :label="t('probes.columns.lastSeen')"
              >
                <span :title="probe.last_seen_at ? dateTime(probe.last_seen_at) : undefined">{{ lastSeen(probe) }}</span>
              </TableCell>
              <TableCell
                class="max-sm:hidden"
                :label="t('probes.columns.lag')"
              >
                <TableNone v-if="probe.revoked_at || (!probe.builtin && !probe.last_seen_at)" />
                <ul
                  v-else-if="probeLag(probe).length > 0"
                  class="grid gap-0.5 text-sm"
                >
                  <li
                    v-for="lag in probeLag(probe)"
                    :key="lag.kind"
                    :class="lag.kind === 'dropped' ? 'text-destructive' : 'text-foreground'"
                  >
                    {{ lagText(lag) }}
                  </li>
                </ul>
                <span
                  v-else
                  class="text-muted"
                >{{ t('probes.onTime') }}</span>
              </TableCell>
              <TableCell
                kind="numeric"
                class="max-sm:hidden"
                :label="t('probes.columns.monitors')"
              >
                {{ number(probe.monitors) }}
              </TableCell>
              <TableCell class="text-right whitespace-nowrap">
                <Button
                  v-if="manage && !probe.revoked_at"
                  variant="ghost"
                  size="icon"
                  :aria-label="t('probes.editNamed', { name: probe.name })"
                  @click="startEdit(probe)"
                >
                  <Icon name="lucide:pencil" />
                </Button>
                <Button
                  v-if="manage && !probe.builtin && !probe.revoked_at"
                  variant="ghost"
                  size="icon"
                  :aria-label="t('probes.revokeNamed', { name: probe.name })"
                  :disabled="revoke.isPending.value"
                  @click="ask(probe)"
                >
                  <Icon name="lucide:trash-2" />
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </section>

      <section
        v-if="inherited.length > 0"
        class="grid gap-3"
        aria-labelledby="probes-inherited-title"
      >
        <div class="grid gap-1">
          <h2
            id="probes-inherited-title"
            class="flex items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
          >
            {{ t('probes.inherited') }}
            <span class="text-sm font-normal text-muted tabular-nums">· {{ inherited.length }}</span>
          </h2>
          <p class="text-sm text-muted">{{ t('probes.inheritedHint') }}</p>
        </div>
        <Table :label="t('probes.inherited')">
          <TableHeader>
            <TableRow>
              <TableHead class="w-32">{{ t('probes.columns.status') }}</TableHead>
              <TableHead>{{ t('probes.columns.name') }}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="probe in inherited"
              :key="probe.id"
              :data-probe="probe.name"
            >
              <TableCell :label="t('probes.columns.status')">
                <ProbeStatusLabel :status="probe.status" />
              </TableCell>
              <TableCell kind="subject">
                <div class="grid min-w-0 gap-1">
                  <span class="truncate">{{ probe.name }}</span>
                  <ProbePlace
                    :location="probe.location"
                    class="text-sm font-normal text-muted"
                  />
                </div>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </section>
    </div>

    <CreateProbeDialog
      v-if="routeId"
      v-model:open="creating"
      :node="routeId"
    />
    <EditProbeDialog
      v-model:open="editing"
      :probe="edited"
      @saved="(probe) => (notice = t('probes.saved', { name: probe.name }))"
    />
  </div>
</template>
