<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'
import { useNow } from '@vueuse/core'

import { incidentsResource } from '~/api/resources/incidents'
import ImpactLabel from '~/components/incidents/ImpactLabel.vue'
import IncidentStateBadge from '~/components/incidents/IncidentStateBadge.vue'
import RailSection from '~/components/observe/RailSection.vue'
import { Button } from '~/components/ui/button'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useLiveSnapshot } from '~/composables/useLive'
import type { MonitorId, NodeId } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * The newest open incident that names the monitor, live, with a way to its page and to resolve it.
 * `scope` is the highest node the caller can read above the monitor, so incidents filed on a node
 * above it count too. Shows nothing while none is open.
 */
const props = defineProps<{
  monitor: MonitorId
  scope: NodeId | null
  /** Whether the caller may manage incidents on a node. */
  manages: (node: NodeId) => boolean
}>()

const SHOWN = 5
const { t } = useI18n()
const { message } = useErrorMessage()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 10_000 })

const open = useLiveSnapshot({
  key: () => (props.scope ? `${props.scope}:${props.monitor}` : null),
  load: (_, signal) =>
    incidentsResource.list({ node: props.scope ?? '', subtree: true, status: 'open', monitor: props.monitor, limit: SHOWN }, signal),
  affects: (event) => event.kind.startsWith('incident.'),
})
const incidents = computed(() => open.data.value?.incidents ?? [])
const incident = computed(() => incidents.value[0] ?? null)
const more = computed(() => incidents.value.length - 1)

const resolve = useMutation({
  mutationFn: (id: string) => incidentsResource.addUpdate({ incident: id, state: 'resolved' }),
})
</script>

<template>
  <RailSection
    v-if="incident"
    id="monitor-incident"
    :title="t('monitors.detail.incident')"
    :meta="more > 0 ? t('monitors.detail.incidentsMore', { count: more }) : undefined"
  >
    <div class="grid gap-2.5 rounded-control border border-status-down/30 bg-status-down/8 px-3.5 py-3 [--muted:color-mix(in_oklab,var(--primary)_72%,transparent)]">
      <RouterLink
        :to="{ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } }"
        class="rounded-sm text-sm font-semibold text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ incident.title }}
      </RouterLink>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[0.8125rem] text-muted">
        <ImpactLabel
          v-if="incident.impact"
          :impact="incident.impact"
        />
        <IncidentStateBadge :state="incident.state" />
        <span :title="dateTime(incident.started_at)">
          {{ t('monitors.detail.incidentStarted', { time: relative(incident.started_at, now.getTime()) }) }}
        </span>
      </div>
      <div class="flex flex-wrap gap-2">
        <Button
          as-child
          variant="accent"
          size="sm"
        >
          <RouterLink :to="{ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } }">
            {{ t('monitors.detail.postUpdate') }}
          </RouterLink>
        </Button>
        <Button
          v-if="manages(incident.node)"
          variant="outline"
          size="sm"
          :loading="resolve.isPending.value"
          @click="resolve.mutate(incident.id)"
        >
          {{ t('incidents.resolve') }}
        </Button>
      </div>
      <p
        v-if="resolve.error.value"
        class="text-[0.8125rem] text-destructive"
        role="alert"
      >
        {{ message(resolve.error.value) }}
      </p>
    </div>
  </RailSection>
</template>
