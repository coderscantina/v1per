<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { incidentsResource } from '~/api/resources/incidents'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import TextareaField from '~/components/TextareaField.vue'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { IncidentDetail, IncidentState } from '~/generated'
import { changedImpacts, currentImpacts, incidentStates, stateLabels, targetKey, type ImpactSelection } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

import ImpactSelect from './ImpactSelect.vue'

/**
 * Posts an update: a state, a message and the impact of each affected target. Only the
 * impacts the person changes are sent; resolving sets every target to operational.
 */
const props = defineProps<{ detail: IncidentDetail }>()

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const state = ref<IncidentState>(props.detail.incident.state === 'resolved' ? 'investigating' : props.detail.incident.state)
const message = ref('')
const chosen = ref<ImpactSelection>(currentImpacts(props.detail.affected))
// Another update moves the starting point; what the person typed stays.
watch(
  () => props.detail.affected,
  (affected) => (chosen.value = { ...currentImpacts(affected), ...chosen.value })
)

const post = useMutation({
  mutationFn: () =>
    incidentsResource.addUpdate({
      incident: props.detail.incident.id,
      state: state.value,
      message: message.value.trim() || undefined,
      impacts: changedImpacts(props.detail.affected, chosen.value, state.value),
    }),
  onSuccess: (result) => {
    message.value = ''
    chosen.value = currentImpacts(props.detail.affected)
    if (result.incident.state === 'resolved') state.value = 'investigating'
  },
})

const stateOptions = computed(() => incidentStates.map((value) => ({ value, label: t(stateLabels[value]) })))
</script>

<template>
  <form
    class="grid gap-4"
    novalidate
    @submit.prevent="post.mutate()"
  >
    <div class="grid gap-1.5">
      <span class="text-sm font-medium text-primary">{{ t('incidents.update.state') }}</span>
      <ChoiceGroup
        v-model="state"
        :label="t('incidents.update.state')"
        :options="stateOptions"
      />
    </div>
    <TextareaField
      id="incident-update-message"
      v-model="message"
      :rows="3"
      :label="t('incidents.update.message')"
      :description="t('incidents.update.messageHint')"
      :error="fieldMessage(post.error.value, 'message')"
    />
    <div
      v-if="state !== 'resolved' && detail.affected.length > 0"
      class="grid gap-2"
    >
      <p class="text-sm font-medium text-primary">{{ t('incidents.update.impacts') }}</p>
      <ul class="grid divide-y divide-divider rounded-lg border border-hairline">
        <li
          v-for="entry in detail.affected"
          :key="targetKey(entry.target)"
          class="flex flex-wrap items-center justify-between gap-2 px-3 py-2"
        >
          <span class="min-w-0 truncate text-sm text-primary">{{ entry.name }}</span>
          <ImpactSelect
            v-model="chosen[targetKey(entry.target)]"
            :label="t('incidents.targets.impactOf', { name: entry.name })"
            :placeholder="t('incidents.update.noImpact')"
          />
        </li>
      </ul>
    </div>
    <p
      v-else-if="state === 'resolved'"
      class="text-sm text-muted"
    >
      {{ t('incidents.update.resolvedHint') }}
    </p>
    <CommandAlert
      :error="post.error.value"
      :shown="['message']"
    />
    <div>
      <Button
        type="submit"
        variant="accent"
        :loading="post.isPending.value"
      >
        {{ t('incidents.update.post') }}
      </Button>
    </div>
  </form>
</template>
