<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { nodesResource } from '~/api/resources/nodes'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import ChildNodes from '~/components/nodes/ChildNodes.vue'
import DeleteNodeDialog from '~/components/nodes/DeleteNodeDialog.vue'
import MoveNodeDialog from '~/components/nodes/MoveNodeDialog.vue'
import NodePath from '~/components/nodes/NodePath.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { Skeleton } from '~/components/ui/skeleton'
import { Switch } from '~/components/ui/switch'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useReauth } from '~/composables/useReauth'
import type { NodeDeleted, NodesRenameArgs } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { can } from '~/lib/access'
import { isReadable } from '~/lib/node-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's name, its children, its 2FA requirement, its place in the tree and its deletion. Each
 * section says which role it needs when the caller's role does not reach.
 */
const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()
const reauth = useReauth()
const router = useRouter()
const { routeId, path, tree } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
usePageMeta(() => t('nodes.settings.title'))

const root = computed(() => node.value?.parent === undefined)
const ancestors = computed(() => path.value.slice(0, -1))

const name = ref('')
watch(
  () => node.value?.name,
  (value) => {
    name.value = value ?? ''
  },
  { immediate: true }
)
const rename = useMutation({ mutationFn: (args: NodesRenameArgs) => nodesResource.rename(args) })
const renamed = ref(false)
const submitRename = () => {
  if (!node.value) return
  renamed.value = false
  rename.mutate({ node: node.value.id, name: name.value }, { onSuccess: () => (renamed.value = true) })
}

const twoFactorBusy = ref(false)
const twoFactorError = ref<unknown>(null)
const setTwoFactor = async (required: boolean) => {
  const current = node.value
  if (!current) return
  twoFactorBusy.value = true
  twoFactorError.value = null
  const args = { node: current.id, required }
  try {
    const result = await reauth.run('nodes.requireTwoFactor', args, () => nodesResource.requireTwoFactor(args))
    if (result) await detail.reload()
  } catch (error) {
    twoFactorError.value = error
  } finally {
    twoFactorBusy.value = false
  }
}

const moving = ref(false)
const deleting = ref(false)

const onDeleted = (result: NodeDeleted) => {
  const parent = node.value?.parent
  const readable = parent ? tree.index.value.byId.get(parent) : undefined
  void router.replace(
    readable && isReadable(readable) ? { name: 'node', params: { nodeId: readable.id } } : { name: 'home' }
  )
  return result
}
</script>

<template>
  <div>
    <PageHeader
      :title="t('nodes.settings.title')"
      :description="node?.name"
    />

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else-if="!node"
      class="grid max-w-4xl gap-6"
    >
      <Skeleton class="h-32 w-full rounded-panel" />
      <Skeleton class="h-32 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="max-w-4xl"
    >
      <SettingsPage>
        <SettingsSection
          id="node-name"
          :title="t('nodes.settings.name')"
          :description="t('nodes.settings.nameDescription')"
        >
          <form
            id="rename-form"
            class="grid gap-4"
            novalidate
            @submit.prevent="submitRename"
          >
            <FormField
              id="rename-name"
              v-model="name"
              autocomplete="off"
              :label="t('nodes.name')"
              :description="can(node, 'node.write') ? undefined : t('nodes.settings.needsAdmin')"
              :maxlength="NAME_MAX_CHARS"
              :error="fieldMessage(rename.error.value, 'name')"
              :readonly="!can(node, 'node.write')"
              required
            />
            <CommandAlert
              :error="rename.error.value"
              :shown="['name']"
            />
          </form>
          <template
            v-if="can(node, 'node.write')"
            #footer
          >
            <Button
              variant="primary"
              type="submit"
              form="rename-form"
              :loading="rename.isPending.value"
              :disabled="name.trim() === '' || name.trim() === node.name"
            >
              {{ t('actions.save') }}
            </Button>
            <span
              v-if="renamed && !rename.isPending.value"
              role="status"
              class="text-sm text-muted"
            >{{ t('actions.saved') }}</span>
          </template>
        </SettingsSection>

        <SettingsSection
          id="node-children"
          :title="t('nodes.children')"
          :description="t('nodes.settings.childrenDescription')"
        >
          <ChildNodes :node="node" />
        </SettingsSection>

        <SettingsSection
          id="node-two-factor"
          :title="t('nodes.settings.twoFactor')"
          :description="t('nodes.settings.twoFactorDescription')"
        >
          <div class="flex items-start gap-3">
            <Switch
              id="require-two-factor"
              :model-value="node.two_factor_required"
              :disabled="!can(node, 'members.manage') || twoFactorBusy"
              @update:model-value="setTwoFactor"
            />
            <div class="grid gap-0.5">
              <Label for="require-two-factor">{{ t('nodes.settings.requireTwoFactor') }}</Label>
              <p class="text-sm text-muted">
                {{
                  !can(node, 'members.manage')
                    ? t('nodes.settings.needsAdmin')
                    : node.two_factor_inherited
                      ? t('nodes.settings.twoFactorInherited')
                      : t('nodes.settings.twoFactorHint')
                }}
              </p>
            </div>
          </div>
          <CommandAlert :error="twoFactorError" />
        </SettingsSection>

        <SettingsSection
          id="node-move"
          :title="t('nodes.settings.place')"
          :description="t('nodes.settings.placeDescription')"
        >
          <p class="text-sm text-muted">
            <span v-if="root">{{ t('nodes.root') }}</span>
            <span
              v-else
              class="flex flex-wrap items-center gap-x-1.5"
            >
              {{ t('nodes.settings.under') }}
              <NodePath :path="ancestors" />
            </span>
          </p>
          <p
            v-if="!root && !can(node, 'node.move')"
            class="text-sm text-muted"
          >
            {{ t('nodes.settings.needsOwner') }}
          </p>
          <template
            v-if="!root && can(node, 'node.move')"
            #footer
          >
            <Button
              variant="fill"
              @click="moving = true"
            >
              <Icon name="lucide:folder-input" />
              {{ t('nodes.move.open') }}
            </Button>
          </template>
        </SettingsSection>

        <SettingsSection
          v-if="!root"
          id="node-delete"
          :title="t('nodes.delete.section')"
          :description="t('nodes.delete.description')"
          destructive
        >
          <p
            v-if="!can(node, 'node.delete')"
            class="text-sm text-muted"
          >
            {{ t('nodes.settings.needsOwner') }}
          </p>
          <template
            v-if="can(node, 'node.delete')"
            #footer
          >
            <Button
              variant="destructive"
              @click="deleting = true"
            >
              <Icon name="lucide:trash-2" />
              {{ t('nodes.delete.open') }}
            </Button>
          </template>
        </SettingsSection>
      </SettingsPage>

      <MoveNodeDialog
        v-model:open="moving"
        :node="node"
      />
      <DeleteNodeDialog
        v-model:open="deleting"
        :node="node"
        @deleted="onDeleted"
      />
    </div>
  </div>
</template>
