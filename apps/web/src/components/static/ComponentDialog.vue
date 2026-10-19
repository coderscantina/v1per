<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { componentsResource } from '~/api/resources/components'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import TextareaField from '~/components/TextareaField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { Component, NodeId } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/** Adds a static component to the node, or renames one. The list updates from the node's topic. */
const props = defineProps<{ node: NodeId; component: Component | null }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const name = ref('')
const description = ref('')

const save = useMutation({
  mutationFn: () => {
    const args = { name: name.value.trim(), description: description.value.trim() || undefined }
    return props.component
      ? componentsResource.update({ component: props.component.id, ...args })
      : componentsResource.create({ node: props.node, ...args })
  },
  onSuccess: () => (open.value = false),
})

watch(open, (value) => {
  if (!value) return
  name.value = props.component?.name ?? ''
  description.value = props.component?.description ?? ''
  save.reset()
})
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-lg">
      <DialogHeaderCombined
        :title="component ? t('components.editTitle') : t('components.create')"
        :description="t('components.dialogDescription')"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="save.mutate()"
      >
        <FormField
          id="component-name"
          v-model="name"
          autocomplete="off"
          :label="t('components.name')"
          :description="t('nodes.nameHint', { max: NAME_MAX_CHARS })"
          :error="fieldMessage(save.error.value, 'name')"
          required
        />
        <TextareaField
          id="component-description"
          v-model="description"
          :rows="3"
          :label="t('components.description')"
          :error="fieldMessage(save.error.value, 'description')"
        />
        <CommandAlert
          :error="save.error.value"
          :shown="['name', 'description']"
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
            :loading="save.isPending.value"
            :disabled="name.trim() === ''"
          >
            {{ component ? t('actions.save') : t('components.create') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
