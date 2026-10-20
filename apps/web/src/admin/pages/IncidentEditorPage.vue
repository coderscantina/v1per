<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { incidentsResource } from '~/api/resources/incidents'
import CommandAlert from '~/components/CommandAlert.vue'
import AffectsPicker from '~/components/incidents/AffectsPicker.vue'
import IncidentComposer from '~/components/incidents/IncidentComposer.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useIncidentDetail, useIncidentTargets } from '~/composables/useIncidents'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { Incident } from '~/generated'
import { can } from '~/lib/access'
import { parseTargetKey, targetKey, type TargetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * Opens an incident with the composer, or changes an incident's title and affected targets.
 * Impacts of an open incident change through updates, on its page.
 */
const { t } = useI18n()
const { fieldMessage, fieldProblem } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId, tree } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'incidents.manage'))

const incidentId = computed(() => (typeof route.params.incidentId === 'string' ? route.params.incidentId : null))
const editing = computed(() => incidentId.value !== null)
const existing = useIncidentDetail(incidentId)
const stored = computed(() => existing.data.value ?? null)
usePageMeta(() => (editing.value ? t('incidents.editor.editTitle') : t('incidents.editor.createTitle')))

const targets = useIncidentTargets(() => (editing.value ? routeId.value : null))
const nodeName = (id: string) => (id === routeId.value ? undefined : tree.index.value.byId.get(id)?.name)

const title = ref('')
const affected = ref<TargetKey[]>([])
const basedOn = ref<number | null>(null)

const load = () => {
  const detail = stored.value
  if (!detail) return
  title.value = detail.incident.title
  affected.value = detail.affected.map((entry) => targetKey(entry.target))
  basedOn.value = detail.incident.revision
}
watch(
  stored,
  () => {
    if (basedOn.value === null) load()
  },
  { immediate: true }
)

const submitted = ref(false)
const titleError = computed(() =>
  submitted.value && title.value.trim() === '' ? fieldProblem({ code: 'required' }) : fieldMessage(save.error.value, 'title')
)

const save = useMutation({
  mutationFn: () =>
    incidentsResource.update({
      incident: incidentId.value ?? '',
      expected_revision: basedOn.value ?? undefined,
      title: title.value.trim(),
      affected: affected.value.map(parseTargetKey),
    }),
})

const open = (incident: Incident) =>
  void router.push({ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } })

const submit = () => {
  submitted.value = true
  if (titleError.value) return
  save.mutate(undefined, { onSuccess: ({ incident }) => open(incident) })
}

const reload = async () => {
  await existing.reload()
  basedOn.value = null
  load()
  save.reset()
}

const ready = computed(() => node.data.value && (!editing.value || basedOn.value !== null))
const backTo = computed(() =>
  incidentId.value
    ? { name: 'node-incident', params: { nodeId: routeId.value, incidentId: incidentId.value } }
    : { name: 'node-incidents', params: { nodeId: routeId.value } }
)
</script>

<template>
  <div>
    <PageHeader
      :title="editing ? t('incidents.editor.editTitle') : t('incidents.editor.createTitle')"
      :description="stored?.incident.title ?? node.data.value?.node.name"
    />

    <Alert
      v-if="(node.error.value && !node.data.value) || (existing.error.value && !stored)"
      color="destructive"
      class="max-w-2xl"
    >
      <p class="text-sm">{{ t('incidents.unavailable') }}</p>
    </Alert>
    <Skeleton
      v-else-if="!ready"
      class="h-44 w-full max-w-2xl rounded-panel"
    />
    <Alert
      v-else-if="!manage"
      icon="lucide:info"
      class="max-w-2xl"
    >
      <p class="text-sm">{{ t('incidents.readOnly') }}</p>
    </Alert>

    <div
      v-else-if="!editing && routeId"
      class="grid max-w-2xl gap-3"
    >
      <IncidentComposer
        :node="routeId"
        autofocus
        @posted="open"
      />
      <div>
        <Button
          as-child
          variant="ghost"
          size="sm"
        >
          <RouterLink :to="backTo">{{ t('actions.cancel') }}</RouterLink>
        </Button>
      </div>
    </div>

    <form
      v-else
      class="grid max-w-2xl gap-3"
      novalidate
      @submit.prevent="submit"
    >
      <div class="grid gap-3 rounded-panel border border-hairline bg-card px-4.5 py-4 max-sm:px-4">
        <input
          id="incident-title"
          v-model="title"
          autocomplete="off"
          class="w-full bg-transparent text-base font-semibold text-primary outline-none placeholder:font-medium placeholder:text-muted"
          :placeholder="t('incidents.composer.titlePlaceholder')"
          :aria-label="t('incidents.editor.title')"
          :aria-invalid="titleError ? true : undefined"
          :aria-describedby="titleError ? 'incident-title-error' : undefined"
        >
        <p
          v-if="titleError"
          id="incident-title-error"
          class="text-xs font-medium text-destructive"
        >
          {{ titleError }}
        </p>
        <Skeleton
          v-if="targets.loading.value"
          class="h-7 w-28"
        />
        <AffectsPicker
          v-else
          v-model="affected"
          :options="targets.options.value"
          :node-name="nodeName"
          :error="fieldMessage(save.error.value, 'affected')"
        />
      </div>
      <CommandAlert
        :error="save.error.value"
        :shown="['title', 'affected']"
        @reload="reload"
      />
      <div class="flex flex-wrap gap-2">
        <Button
          type="submit"
          variant="accent"
          :loading="save.isPending.value"
        >
          {{ t('incidents.editor.saveEdit') }}
        </Button>
        <Button
          as-child
          variant="ghost"
        >
          <RouterLink :to="backTo">{{ t('actions.cancel') }}</RouterLink>
        </Button>
      </div>
    </form>
  </div>
</template>
