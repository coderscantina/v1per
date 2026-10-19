<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { incidentsResource } from '~/api/resources/incidents'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import TargetImpactPicker from '~/components/incidents/TargetImpactPicker.vue'
import PageHeader from '~/components/PageHeader.vue'
import TextareaField from '~/components/TextareaField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useIncidentDetail, useIncidentTargets } from '~/composables/useIncidents'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { IncidentResult, IncidentState } from '~/generated'
import { can } from '~/lib/access'
import {
  incidentStates,
  parseTargetKey,
  stateLabels,
  targetKey,
  toImpacts,
  type ImpactSelection,
  type TargetKey,
} from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * Opens an incident on the node with its first update (state, message and the impact on each
 * affected monitor or static component), or changes an incident's title and affected targets.
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

const targets = useIncidentTargets(routeId)
const nodeName = (id: string) => (id === routeId.value ? undefined : tree.index.value.byId.get(id)?.name)

const title = ref('')
const state = ref<IncidentState>('investigating')
const message = ref('')
const selection = ref<ImpactSelection>({})
const basedOn = ref<number | null>(null)

const load = () => {
  const detail = stored.value
  if (!detail) return
  title.value = detail.incident.title
  selection.value = Object.fromEntries(detail.affected.map((entry) => [targetKey(entry.target), entry.impact ?? 'operational']))
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
const clientErrors = computed(() => {
  if (!submitted.value) return {}
  const errors: Record<string, string> = {}
  if (title.value.trim() === '') errors.title = fieldProblem({ code: 'required' })
  if (!editing.value && Object.keys(selection.value).length === 0) errors.impacts = fieldProblem({ code: 'required' })
  return errors
})
const errorFor = (key: string): string | null => clientErrors.value[key] ?? fieldMessage(save.error.value, key)

const save = useMutation({
  mutationFn: (): Promise<IncidentResult> => {
    const incident = incidentId.value
    if (incident) {
      return incidentsResource.update({
        incident,
        expected_revision: basedOn.value ?? undefined,
        title: title.value.trim(),
        affected: (Object.keys(selection.value) as TargetKey[]).map(parseTargetKey),
      })
    }
    return incidentsResource.create({
      node: routeId.value ?? '',
      title: title.value.trim(),
      state: state.value,
      message: message.value.trim() || undefined,
      impacts: toImpacts(selection.value),
    })
  },
})

const submit = () => {
  submitted.value = true
  if (Object.keys(clientErrors.value).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())
    return
  }
  save.mutate(undefined, {
    onSuccess: ({ incident }) =>
      void router.push({ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } }),
  })
}

const reload = async () => {
  await existing.reload()
  basedOn.value = null
  load()
  save.reset()
}

const stateOptions = computed(() =>
  incidentStates.filter((value) => value !== 'resolved').map((value) => ({ value, label: t(stateLabels[value]) }))
)
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
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('incidents.unavailable') }}</p>
    </Alert>
    <div
      v-else-if="!ready"
      class="grid max-w-4xl gap-6"
    >
      <Skeleton class="h-32 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>
    <Alert
      v-else-if="!manage"
      icon="lucide:info"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('incidents.readOnly') }}</p>
    </Alert>

    <form
      v-else
      class="max-w-4xl"
      novalidate
      @submit.prevent="submit"
    >
      <SettingsPage>
        <SettingsSection
          id="incident-basics"
          :title="t('incidents.editor.basics')"
        >
          <FormField
            id="incident-title"
            v-model="title"
            autocomplete="off"
            :label="t('incidents.editor.title')"
            :description="t('incidents.editor.titleHint')"
            :error="errorFor('title')"
            required
          />
          <template v-if="!editing">
            <div class="grid gap-1.5">
              <span class="text-sm font-medium text-primary">{{ t('incidents.update.state') }}</span>
              <ChoiceGroup
                v-model="state"
                :label="t('incidents.update.state')"
                :options="stateOptions"
              />
            </div>
            <TextareaField
              id="incident-message"
              v-model="message"
              :rows="4"
              :label="t('incidents.update.message')"
              :description="t('incidents.update.messageHint')"
              :error="errorFor('message')"
            />
          </template>
        </SettingsSection>

        <SettingsSection
          id="incident-targets"
          :title="t('incidents.editor.affected')"
          :description="editing ? t('incidents.editor.affectedEditHint') : t('incidents.editor.affectedHint')"
        >
          <Skeleton
            v-if="targets.loading.value"
            class="h-40 w-full"
          />
          <TargetImpactPicker
            v-else
            v-model="selection"
            :legend="t('incidents.editor.affected')"
            :options="targets.options.value"
            :impact="!editing"
            :node-name="nodeName"
            :error="errorFor(editing ? 'affected' : 'impacts')"
          />
        </SettingsSection>
      </SettingsPage>

      <div class="mt-8 grid gap-4 border-t border-divider pt-6">
        <CommandAlert
          :error="save.error.value"
          :shown="['title', 'message', 'impacts', 'affected']"
          @reload="reload"
        />
        <div class="flex flex-wrap gap-2">
          <Button
            type="submit"
            variant="accent"
            :loading="save.isPending.value"
          >
            {{ editing ? t('incidents.editor.saveEdit') : t('incidents.editor.saveCreate') }}
          </Button>
          <Button
            as-child
            variant="fill"
          >
            <RouterLink :to="backTo">{{ t('actions.cancel') }}</RouterLink>
          </Button>
        </div>
      </div>
    </form>
  </div>
</template>
