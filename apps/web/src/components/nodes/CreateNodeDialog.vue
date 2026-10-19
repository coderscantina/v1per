<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { nodesResource } from '~/api/resources/nodes'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { NodeId, NodesCreateArgs } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/** Adds a child node. The tree and the parent's child list update from their topics. */
const props = defineProps<{ parent: NodeId; parentName: string }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const name = ref('')

const create = useMutation({
  mutationFn: (args: NodesCreateArgs) => nodesResource.create(args),
  onSuccess: () => {
    open.value = false
  },
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  create.reset()
})

const submit = () => create.mutate({ parent: props.parent, name: name.value })
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogHeaderCombined
        :title="t('nodes.create.title')"
        :description="t('nodes.create.description', { parent: parentName })"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="submit"
      >
        <FormField
          id="node-name"
          v-model="name"
          autocomplete="off"
          :label="t('nodes.name')"
          :description="t('nodes.nameHint', { max: NAME_MAX_CHARS })"
          :error="fieldMessage(create.error.value, 'name') ?? fieldMessage(create.error.value, 'parent')"
          required
        />
        <CommandAlert
          :error="create.error.value"
          :shown="['name', 'parent']"
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
            :loading="create.isPending.value"
            :disabled="name.trim() === ''"
          >
            {{ t('nodes.create.submit') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
