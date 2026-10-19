<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/account/ApiKeyDialog.vue. A key
// reaches one node and its subtree with a set of capabilities, never more than its owner holds.
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import SecretReveal from '~/components/account/SecretReveal.vue'
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import NodeTreeList from '~/components/nodes/NodeTreeList.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode } from '~/composables/useNodes'
import { useReauth } from '~/composables/useReauth'
import type { ApiKeyCreated, ApiKeysCreateArgs, Capability, TreeNode } from '~/generated'
import { API_KEY_SCHEME, NAME_MAX_CHARS } from '~/generated/limits'
import { keyCapabilityOptions } from '~/lib/access'
import { pathTo } from '~/lib/node-tree'
import { capabilityLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Creates a key in two steps: choose its node and what it may do, then copy it. The second step
 * is the only time the secret exists outside its hash, so the dialog says so and does not close
 * on an outside click.
 */
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const reauth = useReauth()
const current = useCurrentNode()
const index = computed(() => current.tree.index.value)

const name = ref('')
const node = ref<TreeNode | null>(null)
const capabilities = ref<Capability[]>(['node.read'])
const created = ref<ApiKeyCreated | null>(null)

const create = useMutation({
  mutationFn: (args: ApiKeysCreateArgs) => reauth.run('apiKeys.create', args, () => accountResource.createApiKey(args)),
  onSuccess: (result) => {
    if (!result) return
    created.value = result
    void queryClient.invalidateQueries({ queryKey: queryKeys.apiKeys })
  },
})

watch(
  open,
  (value) => {
    if (value) {
      node.value = current.node.value
      return
    }
    name.value = ''
    capabilities.value = ['node.read']
    created.value = null
    create.reset()
  },
  { immediate: true }
)

const options = computed(() =>
  keyCapabilityOptions(node.value?.capabilities ?? []).map(({ capability, held }) => ({
    value: capability,
    label: t(capabilityLabels[capability]),
    disabled: !held,
    hint: held ? undefined : t('apiKeys.notHeld'),
  }))
)

/** A capability the newly picked node does not grant leaves the selection. */
watch(node, (next) => {
  capabilities.value = capabilities.value.filter((capability) => next?.capabilities.includes(capability))
})

const where = computed(() =>
  node.value
    ? pathTo(index.value, node.value.id)
        .map((entry) => entry.name ?? '…')
        .join(' / ')
    : t('apiKeys.noNode')
)
const ready = computed(() => name.value.trim() !== '' && node.value !== null && capabilities.value.length > 0)

const submit = () => {
  if (!node.value) return
  create.mutate({ name: name.value, node: node.value.id, capabilities: capabilities.value })
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-2xl"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('apiKeys.createdTitle')"
          :description="t('apiKeys.createdDescription', { scheme: API_KEY_SCHEME })"
        />
        <SecretReveal
          id="api-key-secret"
          :label="t('apiKeys.secret')"
          :secret="created.secret"
        />
        <div class="flex justify-end">
          <Button
            variant="primary"
            @click="open = false"
          >
            {{ t('actions.done') }}
          </Button>
        </div>
      </template>
      <template v-else>
        <DialogHeaderCombined
          :title="t('apiKeys.create')"
          :description="t('apiKeys.createDescription')"
        />
        <form
          class="grid gap-5"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="api-key-name"
            v-model="name"
            autocomplete="off"
            :label="t('apiKeys.name')"
            :description="t('apiKeys.nameHint', { max: NAME_MAX_CHARS })"
            :error="fieldMessage(create.error.value, 'name')"
            required
          />
          <div class="grid gap-1.5">
            <p
              id="api-key-node-label"
              class="text-sm font-medium text-primary"
            >
              {{ t('apiKeys.node') }}
            </p>
            <p class="text-xs text-muted">{{ t('apiKeys.nodeHint', { node: where }) }}</p>
            <div class="overflow-hidden rounded-lg border border-hairline">
              <NodeTreeList
                :index="index"
                :label="t('apiKeys.node')"
                :selected="node?.id"
                @select="(picked) => (node = picked)"
              />
            </div>
          </div>
          <CheckboxGroup
            v-model="capabilities"
            :legend="t('apiKeys.capabilities')"
            :options="options"
            :error="fieldMessage(create.error.value, 'capabilities')"
          />
          <CommandAlert
            :error="create.error.value"
            :shown="['name', 'capabilities']"
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
              :disabled="!ready"
            >
              {{ t('apiKeys.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
