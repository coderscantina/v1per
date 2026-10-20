<script setup lang="ts">
import IncidentCard from '~/components/incidents/IncidentCard.vue'
import IncidentComposer from '~/components/incidents/IncidentComposer.vue'
import { Alert } from '~/components/ui/alert'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useOpenIncidents } from '~/composables/useIncidents'
import { useNodeDetail } from '~/composables/useNodes'
import { usePageEditor, usePagePreview } from '~/composables/usePages'
import { can } from '~/lib/access'
import { openFirst, targetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * The incidents published on this page, open ones first, read from the page as the team sees
 * it, and for members who manage incidents a composer that opens one on what the page shows.
 * The server publishes a new incident to the pages showing its targets, this one included.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const editor = usePageEditor()
const page = computed(() => editor.data.value?.page ?? null)
const nodeId = computed(() => page.value?.node ?? null)
const node = useNodeDetail(nodeId)
const manage = computed(() => can(node.data.value?.node, 'incidents.manage'))

const shown = computed(() => (editor.data.value?.groups ?? []).flatMap((group) => group.components.map((entry) => targetKey(entry.target))))

// The page's own revision does not move with incidents; the node's incident topic does. Both
// only rise, so their sum moves whenever either does.
const incidentFeed = useOpenIncidents(() => nodeId.value ?? '', 1)
const preview = usePagePreview(
  () => page.value?.id ?? '',
  'internal',
  () => page.value?.default_locale ?? 'en',
  () => (page.value?.revision ?? 0) + (incidentFeed.data.value?.revision ?? 0)
)
const incidents = computed(() => openFirst(preview.data.value?.snapshot.incidents ?? []))
</script>

<template>
  <div
    v-if="page && nodeId"
    class="grid max-w-2xl gap-5"
  >
    <section
      v-if="manage"
      :aria-label="t('pages.incidents.compose')"
    >
      <IncidentComposer
        :node="nodeId"
        :only="shown"
        @posted="() => preview.refetch()"
      />
    </section>

    <Alert
      v-if="preview.error.value && !preview.data.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(preview.error.value) }}</p>
    </Alert>
    <Skeleton
      v-else-if="!preview.data.value"
      class="h-20 w-full rounded-panel"
    />
    <p
      v-else-if="incidents.length === 0"
      class="rounded-panel border border-hairline bg-card px-4.5 py-6 text-center text-sm text-muted"
    >
      {{ t('pages.incidents.none') }}
    </p>
    <ul
      v-else
      class="grid gap-2.5"
      :aria-label="t('pages.incidents.title')"
    >
      <li
        v-for="incident in incidents"
        :key="incident.id"
      >
        <IncidentCard
          :incident="incident"
          :node="nodeId"
          :latest="incident.updates[0]?.message"
        />
      </li>
    </ul>
  </div>
</template>
