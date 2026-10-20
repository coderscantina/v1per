<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { probesResource } from '~/api/resources/probes'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { Probe, ProbeLocation, ProbesUpdateArgs } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

import ProbeLocationFields from './ProbeLocationFields.vue'

/** Renames a probe and sets where it runs. The list updates from the node's topic. */
const props = defineProps<{ probe: Probe | null }>()
const emit = defineEmits<{ saved: [probe: Probe] }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const name = ref('')
const location = ref<ProbeLocation>({})

const update = useMutation({
  mutationFn: (args: ProbesUpdateArgs) => probesResource.update(args),
  onSuccess: ({ probe }) => {
    open.value = false
    emit('saved', probe)
  },
})

watch(open, (value) => {
  if (!value || !props.probe) return
  name.value = props.probe.name
  location.value = { ...props.probe.location }
  update.reset()
})

const submit = () => {
  if (props.probe) update.mutate({ probe: props.probe.id, name: name.value, location: location.value })
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-xl">
      <DialogHeaderCombined
        :title="t('probes.edit')"
        :description="t('probes.editDescription')"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="submit"
      >
        <FormField
          id="probe-edit-name"
          v-model="name"
          autocomplete="off"
          :label="t('probes.name')"
          :maxlength="NAME_MAX_CHARS"
          :error="fieldMessage(update.error.value, 'name')"
          required
        />
        <ProbeLocationFields
          id="probe-edit"
          v-model="location"
          :error="(field) => fieldMessage(update.error.value, `location.${field}`)"
        />
        <CommandAlert
          :error="update.error.value"
          :shown="['name', 'location.country', 'location.city', 'location.provider']"
        />
        <div class="flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            @click="open = false"
          >
            {{ t('actions.cancel') }}
          </Button>
          <Button
            type="submit"
            variant="primary"
            :loading="update.isPending.value"
            :disabled="name.trim() === ''"
          >
            {{ t('actions.save') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
