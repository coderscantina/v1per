<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { incidentsResource } from '~/api/resources/incidents'
import CommandAlert from '~/components/CommandAlert.vue'
import TextareaField from '~/components/TextareaField.vue'
import { Button } from '~/components/ui/button'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { IncidentDetail } from '~/generated'
import { INCIDENT_POSTMORTEM_MAX_CHARS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/**
 * The postmortem of a resolved incident: plain text the status pages show on the incident's own
 * page. Managers write, change and remove it here; saving it empty removes it. Others read it.
 */
const props = defineProps<{ detail: IncidentDetail; manage: boolean }>()

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const { dateTime } = useDateFormat()

const stored = computed(() => props.detail.postmortem?.text ?? '')
const text = ref(stored.value)
// Someone else's save arrives live; take it unless this form holds unsaved text.
watch(stored, (next, previous) => {
  if (text.value === previous) text.value = next
})

const save = useMutation({
  mutationFn: () =>
    incidentsResource.setPostmortem({
      incident: props.detail.incident.id,
      expected_revision: props.detail.incident.revision,
      postmortem: text.value.trim(),
    }),
})
const dirty = computed(() => text.value.trim() !== stored.value)
</script>

<template>
  <section
    class="grid gap-3 rounded-panel border border-hairline bg-card px-4.5 py-5 max-sm:px-4"
    aria-labelledby="incident-postmortem-title"
    data-testid="postmortem"
  >
    <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
      <h2
        id="incident-postmortem-title"
        class="text-base font-semibold text-primary"
      >
        {{ t('incidents.postmortem.title') }}
      </h2>
      <p
        v-if="detail.postmortem"
        class="text-sm text-muted"
      >
        {{ t('incidents.postmortem.savedAt', { time: dateTime(detail.postmortem.published_at) }) }}
      </p>
    </div>
    <form
      v-if="manage"
      class="grid gap-3"
      novalidate
      @submit.prevent="save.mutate()"
    >
      <TextareaField
        id="incident-postmortem"
        v-model="text"
        :label="t('incidents.postmortem.text')"
        :description="t('incidents.postmortem.hint')"
        :rows="8"
        :maxlength="INCIDENT_POSTMORTEM_MAX_CHARS"
        :error="fieldMessage(save.error.value, 'postmortem')"
      />
      <CommandAlert
        :error="save.error.value"
        :shown="['postmortem']"
      />
      <div>
        <Button
          type="submit"
          variant="fill"
          :disabled="!dirty"
          :loading="save.isPending.value"
        >
          {{ stored && text.trim() === '' ? t('incidents.postmortem.remove') : t('incidents.postmortem.save') }}
        </Button>
      </div>
    </form>
    <p
      v-else
      class="text-sm whitespace-pre-line text-primary select-text"
    >
      {{ stored }}
    </p>
  </section>
</template>
