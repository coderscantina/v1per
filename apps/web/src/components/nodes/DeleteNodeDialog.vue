<script setup lang="ts">
import { ApiError } from '~/api'
import { nodesResource } from '~/api/resources/nodes'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useNodeTree } from '~/composables/useNodes'
import { useReauth } from '~/composables/useReauth'
import type { Node, NodeDeleted } from '~/generated'
import { descendantsOf } from '~/lib/node-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * Deletes a node. An empty one takes a plain confirmation; one with nodes below needs its name
 * typed, and the server deletes only if the count it sees matches the one confirmed here.
 */
const props = defineProps<{ node: Node }>()
const open = defineModel<boolean>('open', { default: false })
const emit = defineEmits<{ deleted: [result: NodeDeleted] }>()

const { t } = useI18n()
const reauth = useReauth()
const tree = useNodeTree()

const typed = ref('')
const busy = ref(false)
const error = ref<unknown>(null)
/** The server's count after a refusal, when nodes were added since the tree was read. */
const counted = ref<number | null>(null)

const below = computed(() => counted.value ?? descendantsOf(tree.index.value, props.node.id).length)
const ready = computed(() => below.value === 0 || typed.value.trim() === props.node.name)

watch(open, (value) => {
  if (value) return
  typed.value = ''
  error.value = null
  counted.value = null
})

const submit = async () => {
  busy.value = true
  error.value = null
  const args = { node: props.node.id, descendants: below.value > 0 ? below.value : undefined }
  try {
    const result = await reauth.run('nodes.delete', args, () => nodesResource.delete(args))
    if (!result) return
    open.value = false
    emit('deleted', result)
  } catch (failure) {
    if (failure instanceof ApiError && failure.code === 'node_not_empty' && failure.body.current !== undefined) {
      counted.value = failure.body.current
      typed.value = ''
    }
    error.value = failure
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogHeaderCombined
        :title="t('nodes.delete.title', { name: node.name })"
        :description="below > 0 ? t('nodes.delete.cascade', { count: below }, below) : t('nodes.delete.empty')"
      />
      <form
        class="grid gap-4"
        @submit.prevent="submit"
      >
        <template v-if="below > 0">
          <Alert color="warning">
            <p class="text-sm">{{ t('nodes.delete.warning') }}</p>
          </Alert>
          <FormField
            id="delete-confirm"
            v-model="typed"
            autocomplete="off"
            :label="t('nodes.delete.typeName', { name: node.name })"
            required
          />
        </template>
        <CommandAlert :error="error" />
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
            variant="destructive"
            :loading="busy"
            :disabled="!ready"
          >
            {{ t('nodes.delete.submit') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
