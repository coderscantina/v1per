<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { incidentsResource } from '~/api/resources/incidents'
import CommandAlert from '~/components/CommandAlert.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { Textarea } from '~/components/ui/textarea'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useIncidentTargets } from '~/composables/useIncidents'
import { useNodeTree } from '~/composables/useNodes'
import type { Impact, Incident, IncidentDetail, IncidentResult, IncidentState, NodeId } from '~/generated'
import {
  DEFAULT_IMPACT,
  allAt,
  changedImpacts,
  currentImpacts,
  impactFill,
  impactLabels,
  impacts,
  incidentStates,
  selectTargets,
  sharedImpact,
  stateFill,
  stateLabels,
  targetKey,
  toImpacts,
  type ImpactSelection,
  type TargetKey,
} from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

import AffectsPicker from './AffectsPicker.vue'
import ChoiceChip from './ChoiceChip.vue'
import ImpactSelect from './ImpactSelect.vue'

/**
 * Posts to an incident like a short status post. Without `detail` it opens one on `node`: a
 * title, what is happening, a state, what it affects and one impact for all of it. With
 * `detail` it posts the next update. Impacts per target show only on request; an update sends
 * only the impacts it changes, and resolving sets every target to operational on the server.
 */
const props = defineProps<{
  node: NodeId
  detail?: IncidentDetail
  /** Limits what a new incident may name, such as the targets one status page shows. */
  only?: readonly TargetKey[]
  autofocus?: boolean
}>()
const emit = defineEmits<{ posted: [incident: Incident] }>()

const { t } = useI18n()
const { fieldMessage, fieldProblem } = useErrorMessage()
const tree = useNodeTree()
const creating = computed(() => props.detail === undefined)

const targets = useIncidentTargets(() => (creating.value ? props.node : null))
const options = computed(() => {
  const only = props.only && new Set(props.only)
  return only ? targets.options.value.filter((option) => only.has(option.key)) : targets.options.value
})
const nodeName = (id: string) => (id === props.node ? undefined : tree.index.value.byId.get(id)?.name)

const startState = (): IncidentState => {
  const current = props.detail?.incident.state
  return current === undefined || current === 'resolved' ? 'investigating' : current
}
const title = ref('')
const message = ref('')
const state = ref<IncidentState>(startState())
const advanced = ref(false)
const fallback = ref<Impact>(DEFAULT_IMPACT)
const selection = ref<ImpactSelection>(props.detail ? currentImpacts(props.detail.affected) : {})
// Another update moves the starting point; what the person chose stays.
watch(
  () => props.detail?.affected,
  (affected) => {
    if (affected) selection.value = { ...currentImpacts(affected), ...selection.value }
  }
)

/** The targets the impact chip and the per-target list cover. */
const keys = computed<TargetKey[]>(() =>
  props.detail ? props.detail.affected.map((entry) => targetKey(entry.target)) : (Object.keys(selection.value) as TargetKey[])
)
const picked = computed<TargetKey[]>({
  get: () => keys.value,
  set: (next) => (selection.value = selectTargets(selection.value, next, impact.value ?? fallback.value)),
})
const impact = computed<Impact | undefined>({
  get: () => sharedImpact(selection.value) ?? (keys.value.length === 0 ? fallback.value : undefined),
  set: (next) => {
    if (!next) return
    fallback.value = next
    selection.value = { ...selection.value, ...allAt(keys.value, next) }
  },
})
const mixed = computed(() => Object.keys(selection.value).length > 0 && sharedImpact(selection.value) === undefined)
const names = computed(() =>
  props.detail
    ? props.detail.affected.map((entry) => ({ key: targetKey(entry.target), name: entry.name }))
    : options.value.filter((option) => option.key in selection.value).map((option) => ({ key: option.key, name: option.name }))
)

const stateOptions = computed(() =>
  incidentStates
    .filter((value) => !creating.value || value !== 'resolved')
    .map((value) => ({ value, label: t(stateLabels[value]), fill: stateFill[value] }))
)
const impactOptions = computed(() => impacts.map((value) => ({ value, label: t(impactLabels[value]), fill: impactFill[value] })))
const resolving = computed(() => state.value === 'resolved')

const submitted = ref(false)
const clientErrors = computed(() => {
  if (!submitted.value || !creating.value) return {}
  const errors: Record<string, string> = {}
  if (title.value.trim() === '') errors.title = fieldProblem({ code: 'required' })
  if (keys.value.length === 0) errors.impacts = fieldProblem({ code: 'required' })
  return errors
})
const errorFor = (key: string): string | null => clientErrors.value[key] ?? fieldMessage(post.error.value, key)

const post = useMutation({
  mutationFn: (): Promise<IncidentResult> => {
    const detail = props.detail
    if (detail) {
      return incidentsResource.addUpdate({
        incident: detail.incident.id,
        state: state.value,
        message: message.value.trim() || undefined,
        impacts: changedImpacts(detail.affected, selection.value, state.value),
      })
    }
    return incidentsResource.create({
      node: props.node,
      title: title.value.trim(),
      state: state.value,
      message: message.value.trim() || undefined,
      impacts: toImpacts(selection.value),
    })
  },
  onSuccess: (result) => {
    title.value = ''
    message.value = ''
    submitted.value = false
    advanced.value = false
    selection.value = props.detail ? currentImpacts(props.detail.affected) : {}
    state.value = result.incident.state === 'resolved' || creating.value ? 'investigating' : result.incident.state
    emit('posted', result.incident)
  },
})

const titleInput = useTemplateRef<HTMLInputElement>('titleInput')
const messageInput = useTemplateRef<InstanceType<typeof Textarea>>('messageInput')
const focus = () => {
  if (titleInput.value) titleInput.value.focus()
  else messageInput.value?.$el.focus()
}
defineExpose({ focus })
onMounted(() => {
  if (props.autofocus) focus()
})

const submit = () => {
  submitted.value = true
  if (Object.keys(clientErrors.value).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[data-composer] [aria-invalid="true"]')?.focus())
    return
  }
  post.mutate()
}
</script>

<template>
  <form
    class="rounded-panel border border-hairline bg-card text-card-foreground"
    novalidate
    data-composer
    @submit.prevent="submit"
    @keydown.meta.enter.prevent="submit"
    @keydown.ctrl.enter.prevent="submit"
  >
    <div class="grid gap-2 px-4.5 pt-4 pb-3 max-sm:px-4">
      <template v-if="creating">
        <input
          id="incident-title"
          ref="titleInput"
          v-model="title"
          autocomplete="off"
          class="w-full bg-transparent text-base font-semibold text-primary outline-none placeholder:font-medium placeholder:text-muted"
          :placeholder="t('incidents.composer.titlePlaceholder')"
          :aria-label="t('incidents.editor.title')"
          :aria-invalid="errorFor('title') ? true : undefined"
          :aria-describedby="errorFor('title') ? 'incident-title-error' : undefined"
        >
        <p
          v-if="errorFor('title')"
          id="incident-title-error"
          class="text-xs font-medium text-destructive"
        >
          {{ errorFor('title') }}
        </p>
      </template>
      <Textarea
        id="incident-message"
        ref="messageInput"
        v-model="message"
        :auto-size="320"
        :rows="2"
        class="min-h-12 resize-none rounded-none border-0 bg-transparent px-0 py-0 shadow-none"
        :placeholder="t('incidents.composer.messagePlaceholder')"
        :aria-label="t('incidents.update.message')"
        :aria-invalid="errorFor('message') ? true : undefined"
      />
      <p
        v-if="errorFor('message')"
        class="text-xs font-medium text-destructive"
      >
        {{ errorFor('message') }}
      </p>
      <template v-if="creating">
        <Skeleton
          v-if="targets.loading.value"
          class="h-7 w-28"
        />
        <AffectsPicker
          v-else
          v-model="picked"
          :options="options"
          :node-name="nodeName"
          :error="errorFor('impacts')"
        />
      </template>
    </div>

    <ul
      v-if="advanced && !resolving && names.length > 0"
      class="mx-4.5 mb-3 grid divide-y divide-divider rounded-lg border border-hairline max-sm:mx-4"
      :aria-label="t('incidents.composer.perTarget')"
    >
      <li
        v-for="entry in names"
        :key="entry.key"
        class="flex flex-wrap items-center justify-between gap-2 px-3 py-1.5"
      >
        <span class="min-w-0 truncate text-sm text-primary">{{ entry.name }}</span>
        <ImpactSelect
          v-model="selection[entry.key]"
          :label="t('incidents.targets.impactOf', { name: entry.name })"
          :placeholder="t('incidents.update.noImpact')"
        />
      </li>
    </ul>

    <div class="flex flex-wrap items-center gap-1.5 border-t border-divider px-3 py-2.5">
      <ChoiceChip
        v-model="state"
        :label="t('incidents.update.state')"
        :options="stateOptions"
      />
      <ChoiceChip
        v-if="!resolving && (creating || keys.length > 0)"
        v-model="impact"
        :label="t('incidents.update.impacts')"
        :options="impactOptions"
        :placeholder="mixed ? t('incidents.composer.mixed') : t('incidents.update.noImpact')"
      />
      <Button
        v-if="!resolving && keys.length > 1"
        type="button"
        size="sm"
        variant="ghost"
        class="text-muted"
        :aria-expanded="advanced"
        @click="advanced = !advanced"
      >
        <Icon name="lucide:sliders-horizontal" />
        {{ t('incidents.composer.perTarget') }}
      </Button>
      <Button
        type="submit"
        variant="accent"
        class="ml-auto"
        :loading="post.isPending.value"
      >
        <Icon name="lucide:send" />
        {{ t('incidents.composer.post') }}
      </Button>
    </div>

    <div
      v-if="post.error.value"
      class="px-3 pb-3"
    >
      <CommandAlert
        :error="post.error.value"
        :shown="['title', 'message', 'impacts']"
      />
    </div>
  </form>
</template>
