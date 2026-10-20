<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'
import { useNow } from '@vueuse/core'

import { incidentsResource } from '~/api/resources/incidents'
import { pagesResource } from '~/api/resources/pages'
import Icon from '~/components/Icon.vue'
import ImpactLabel from '~/components/incidents/ImpactLabel.vue'
import IncidentComposer from '~/components/incidents/IncidentComposer.vue'
import IncidentPublications from '~/components/incidents/IncidentPublications.vue'
import IncidentStateBadge from '~/components/incidents/IncidentStateBadge.vue'
import IncidentTimeline from '~/components/incidents/IncidentTimeline.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '~/components/ui/dropdown-menu'
import { EmptyState } from '~/components/ui/empty-state'
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
import { durationParts, targetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * One incident as a feed, live: a header with its state, impact, age and what it affects, the
 * status pages it is on, the composer for the next update, then the updates newest first.
 * Resolve, edit and delete sit in the header. An auto-incident says which monitor opened it
 * and offers triage: post an update, publish to the pages showing it, or dismiss a false alarm.
 */
const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 30_000 })
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

/** "2h 5m" open so far, or how long it lasted once resolved. */
const duration = computed(() => {
  const current = incident.value
  if (!current) return ''
  const end = current.resolved_at ?? now.value.getTime()
  return durationParts(end - current.started_at)
    .map(([unit, value]) => new Intl.NumberFormat(locale.value, { style: 'unit', unit, unitDisplay: 'narrow' }).format(value))
    .join(' ')
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

const composer = useTemplateRef<{ focus: () => void }>('composer')
</script>

<template>
  <div>
    <PageHeader :title="incident?.title ?? t('incidents.title')">
      <template
        v-if="incident && manage"
        #actions
      >
        <Button
          v-if="incident.state !== 'resolved'"
          variant="ghost"
          :disabled="act.isPending.value"
          @click="resolve"
        >
          <Icon name="lucide:check" />
          {{ t('incidents.resolve') }}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button
              variant="ghost"
              size="icon"
              :aria-label="t('incidents.feed.more')"
            >
              <Icon name="lucide:ellipsis" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="end"
            class="w-48"
          >
            <DropdownMenuItem as-child>
              <RouterLink :to="{ name: 'node-incident-edit', params: { nodeId: incident.node, incidentId: incident.id } }">
                <Icon name="lucide:pencil" />
                {{ t('incidents.edit') }}
              </RouterLink>
            </DropdownMenuItem>
            <DropdownMenuItem
              class="text-destructive focus:text-destructive"
              @select="remove(false)"
            >
              <Icon name="lucide:trash-2" />
              {{ t('incidents.delete') }}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
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
      class="grid max-w-3xl gap-4"
    >
      <Skeleton class="h-28 w-full rounded-panel" />
      <Skeleton class="h-40 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-3xl gap-4"
    >
      <Alert
        v-if="act.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(act.error.value) }}</p>
      </Alert>

      <section
        class="grid gap-3"
        :aria-label="t('incidents.status')"
      >
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-sm text-muted">
          <IncidentStateBadge :state="incident.state" />
          <ImpactLabel
            v-if="incident.impact"
            :impact="incident.impact"
          />
          <span :title="dateTime(incident.started_at)">{{ t('incidents.feed.started', { time: relative(incident.started_at, now.getTime()) }) }}</span>
          <span>{{ incident.resolved_at ? t('incidents.feed.lasted', { duration }) : t('incidents.feed.openFor', { duration }) }}</span>
        </div>
        <ul
          v-if="detail.affected.length > 0"
          class="flex flex-wrap gap-1.5"
          :aria-label="t('incidents.editor.affected')"
        >
          <li
            v-for="entry in detail.affected"
            :key="targetKey(entry.target)"
            class="inline-flex h-7 max-w-full items-center gap-2 rounded-md border border-hairline bg-card px-2.5 text-sm text-primary"
            :data-affected="entry.name"
          >
            <Icon
              :name="entry.target.type === 'monitor' ? 'lucide:activity' : 'lucide:box'"
              size="13"
              class="shrink-0 text-muted"
              aria-hidden="true"
            />
            <span class="truncate">{{ entry.name }}</span>
            <ImpactLabel
              v-if="entry.impact"
              :impact="entry.impact"
            />
          </li>
        </ul>
        <p
          v-else
          class="text-sm text-muted"
        >
          {{ t('incidents.noAffected') }}
        </p>
        <IncidentPublications
          :publications="publications.data.value"
          :manage="manage"
          :pending="publishing.isPending.value"
          :error="publishing.error.value ?? publications.error.value"
          @publish="(entry) => publishing.mutate({ entry, publish: true })"
          @unpublish="(entry) => publishing.mutate({ entry, publish: false })"
        />
      </section>

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
              @click="composer?.focus()"
            >
              {{ t('incidents.auto.postUpdate') }}
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

      <section
        v-if="manage"
        :aria-label="t('incidents.update.title')"
      >
        <IncidentComposer
          ref="composer"
          :key="incident.id"
          :node="incident.node"
          :detail="detail"
        />
      </section>

      <section
        class="rounded-panel border border-hairline bg-card px-4.5 py-5 max-sm:px-4"
        :aria-label="t('incidents.timeline.title')"
      >
        <IncidentTimeline
          :updates="detail.updates"
          :names="names"
        />
      </section>
    </div>
  </div>
</template>
