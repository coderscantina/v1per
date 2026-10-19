<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { incidentsResource } from '~/api/resources/incidents'
import { pagesResource } from '~/api/resources/pages'
import Icon from '~/components/Icon.vue'
import ImpactLabel from '~/components/incidents/ImpactLabel.vue'
import IncidentPublications from '~/components/incidents/IncidentPublications.vue'
import IncidentStateBadge from '~/components/incidents/IncidentStateBadge.vue'
import IncidentTimeline from '~/components/incidents/IncidentTimeline.vue'
import IncidentUpdateForm from '~/components/incidents/IncidentUpdateForm.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useIncidentDetail } from '~/composables/useIncidents'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useIncidentPublications } from '~/composables/usePages'
import type { IncidentPublication } from '~/generated'
import { can } from '~/lib/access'
import { targetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * One incident, live: its state and the impact on each affected monitor or static component,
 * the timeline newest first, the status pages it is on, and for members a quick update,
 * resolve, edit, delete and publishing per page. An auto-incident says which monitor opened it
 * and offers triage: update, publish to the pages showing it, or dismiss a false alarm.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { dateTime } = useDateFormat()
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'incidents.manage'))

const incidentId = computed(() => (typeof route.params.incidentId === 'string' ? route.params.incidentId : null))
const live = useIncidentDetail(incidentId)
const detail = computed(() => live.data.value ?? null)
const incident = computed(() => detail.value?.incident ?? null)
usePageMeta(() => incident.value?.title ?? t('incidents.title'))

// A link from a parent node's list may name the parent; the incident's own node scopes the page.
watch(incident, (current) => {
  if (current && current.node !== routeId.value) {
    void router.replace({ name: 'node-incident', params: { nodeId: current.node, incidentId: current.id } })
  }
})

const names = computed(() => new Map((detail.value?.affected ?? []).map((entry) => [targetKey(entry.target), entry.name] as const)))
const autoMonitor = computed(() => {
  const current = detail.value
  const monitor = current?.incident.auto_monitor
  if (!current || !monitor) return null
  const entry = current.affected.find((target) => target.target.type === 'monitor' && target.target.monitor === monitor)
  return { id: monitor, name: entry?.name ?? '' }
})

const act = useMutation({ mutationFn: (action: () => Promise<unknown>) => action() })

// Publishing records an incident event, so the live snapshot's revision moving reads this again.
const publications = useIncidentPublications(incidentId, () => live.data.value?.revision ?? 0)
const publishing = useMutation({
  mutationFn: ({ entry, publish }: { entry: IncidentPublication; publish: boolean }) => {
    const current = incident.value
    if (!current) throw new Error('incident not loaded')
    return publish ? pagesResource.publish(current.id, entry.page) : pagesResource.unpublish(current.id, entry.page)
  },
})
const drafts = computed(() => (publications.data.value?.pages ?? []).filter((entry) => entry.published_at === undefined))
/** Triage: publish to every page showing the incident that does not carry it yet. */
const publishAll = async () => {
  for (const entry of drafts.value) await publishing.mutateAsync({ entry, publish: true }).catch(() => undefined)
}
const resolve = () => {
  const current = incident.value
  if (current) act.mutate(() => incidentsResource.addUpdate({ incident: current.id, state: 'resolved' }))
}
const remove = async (dismiss: boolean) => {
  const current = incident.value
  if (!current) return
  const ok = await confirm({
    title: dismiss ? t('incidents.dismissTitle') : t('incidents.deleteTitle'),
    message: dismiss ? t('incidents.dismissConfirm', { title: current.title }) : t('incidents.deleteConfirm', { title: current.title }),
    confirmLabel: dismiss ? t('incidents.dismiss') : t('incidents.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  act.mutate(() => incidentsResource.delete(current.id), {
    onSuccess: () => void router.replace({ name: 'node-incidents', params: { nodeId: current.node } }),
  })
}
</script>

<template>
  <div>
    <PageHeader
      :title="incident?.title ?? t('incidents.title')"
      :description="incident ? (incident.origin === 'auto' ? t('incidents.origins.auto') : t('incidents.origins.manual')) : undefined"
    >
      <template
        v-if="incident && manage"
        #actions
      >
        <Button
          v-if="incident.state !== 'resolved'"
          variant="fill"
          :disabled="act.isPending.value"
          @click="resolve"
        >
          <Icon name="lucide:check" />
          {{ t('incidents.resolve') }}
        </Button>
        <Button
          as-child
          variant="fill"
        >
          <RouterLink :to="{ name: 'node-incident-edit', params: { nodeId: incident.node, incidentId: incident.id } }">
            <Icon name="lucide:pencil" />
            {{ t('incidents.edit') }}
          </RouterLink>
        </Button>
        <Button
          variant="destructive"
          :disabled="act.isPending.value"
          @click="remove(false)"
        >
          <Icon name="lucide:trash-2" />
          {{ t('incidents.delete') }}
        </Button>
      </template>
      <template
        v-if="incident"
        #freshness
      >
        <LiveIndicator :status="live.status.value" />
      </template>
    </PageHeader>

    <EmptyState
      v-if="live.error.value && !detail"
      icon="lucide:siren"
      tone="error"
      :title="t('incidents.unavailable')"
      :description="message(live.error.value)"
      class="mx-auto max-w-lg"
    >
      <Button
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'node-incidents', params: { nodeId: routeId } }">{{ t('incidents.title') }}</RouterLink>
      </Button>
    </EmptyState>

    <div
      v-else-if="!detail || !incident"
      class="grid max-w-6xl gap-6"
    >
      <Skeleton class="h-36 w-full rounded-panel" />
      <Skeleton class="h-64 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-6xl gap-6"
    >
      <Alert
        v-if="act.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(act.error.value) }}</p>
      </Alert>

      <Alert
        v-if="incident.origin === 'auto'"
        icon="lucide:zap"
        data-testid="auto-incident"
      >
        <div class="grid gap-3">
          <p class="text-sm">
            <template v-if="autoMonitor">
              {{ t('incidents.auto.openedBy') }}
              <RouterLink
                :to="{ name: 'node-monitor', params: { nodeId: incident.node, monitorId: autoMonitor.id } }"
                class="font-medium underline underline-offset-4"
              >{{ autoMonitor.name }}</RouterLink>.
            </template>
            {{ incident.edited ? t('incidents.auto.edited') : t('incidents.auto.resolvesItself') }}
          </p>
          <div
            v-if="manage && incident.state !== 'resolved'"
            class="flex flex-wrap gap-2"
          >
            <Button
              size="sm"
              variant="fill"
              as-child
            >
              <a href="#incident-update-title">{{ t('incidents.auto.postUpdate') }}</a>
            </Button>
            <Button
              v-if="drafts.length > 0"
              size="sm"
              variant="fill"
              :disabled="publishing.isPending.value"
              @click="publishAll"
            >
              <Icon name="lucide:globe" />
              {{ t('incidents.pages.publishAll', { count: drafts.length }, drafts.length) }}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              :disabled="act.isPending.value"
              @click="remove(true)"
            >
              {{ t('incidents.dismiss') }}
            </Button>
          </div>
        </div>
      </Alert>

      <div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
        <Panel
          id="incident-status"
          :title="t('incidents.status')"
        >
          <div class="grid gap-4">
            <div class="flex flex-wrap items-center gap-3">
              <IncidentStateBadge :state="incident.state" />
              <ImpactLabel
                v-if="incident.impact"
                :impact="incident.impact"
              />
            </div>
            <dl class="grid gap-2 text-sm">
              <div class="grid gap-0.5 sm:grid-cols-[8rem_1fr] sm:gap-4">
                <dt class="text-muted">{{ t('incidents.columns.started') }}</dt>
                <dd class="text-primary">{{ dateTime(incident.started_at) }}</dd>
              </div>
              <div
                v-if="incident.resolved_at"
                class="grid gap-0.5 sm:grid-cols-[8rem_1fr] sm:gap-4"
              >
                <dt class="text-muted">{{ t('incidents.resolvedAt') }}</dt>
                <dd class="text-primary">{{ dateTime(incident.resolved_at) }}</dd>
              </div>
            </dl>
            <div class="grid gap-2 border-t border-divider pt-4">
              <h3 class="text-sm font-medium text-primary">{{ t('incidents.editor.affected') }}</h3>
              <ul
                v-if="detail.affected.length > 0"
                class="grid gap-2"
              >
                <li
                  v-for="entry in detail.affected"
                  :key="targetKey(entry.target)"
                  class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1"
                  :data-affected="entry.name"
                >
                  <span class="inline-flex min-w-0 items-center gap-2 text-sm text-primary">
                    <Icon
                      :name="entry.target.type === 'monitor' ? 'lucide:activity' : 'lucide:box'"
                      size="14"
                      class="shrink-0 text-muted"
                      aria-hidden="true"
                    />
                    <span class="truncate">{{ entry.name }}</span>
                  </span>
                  <ImpactLabel
                    v-if="entry.impact"
                    :impact="entry.impact"
                  />
                  <span
                    v-else
                    class="text-sm text-muted"
                  >{{ t('incidents.update.noImpact') }}</span>
                </li>
              </ul>
              <p
                v-else
                class="text-sm text-muted"
              >
                {{ t('incidents.noAffected') }}
              </p>
            </div>
          </div>
        </Panel>

        <Panel
          v-if="manage"
          id="incident-update-title"
          :title="t('incidents.update.title')"
        >
          <IncidentUpdateForm
            :key="incident.id"
            :detail="detail"
          />
        </Panel>
      </div>

      <IncidentPublications
        :publications="publications.data.value"
        :manage="manage"
        :pending="publishing.isPending.value"
        :error="publishing.error.value ?? publications.error.value"
        @publish="(entry) => publishing.mutate({ entry, publish: true })"
        @unpublish="(entry) => publishing.mutate({ entry, publish: false })"
      />

      <Panel
        id="incident-timeline"
        :title="t('incidents.timeline.title')"
        :meta="t('incidents.timeline.count', { count: detail.updates.length }, detail.updates.length)"
      >
        <IncidentTimeline
          :updates="detail.updates"
          :names="names"
        />
      </Panel>
    </div>
  </div>
</template>
