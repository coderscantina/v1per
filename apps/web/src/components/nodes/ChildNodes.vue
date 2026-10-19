<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { nodesResource } from '~/api/resources/nodes'
import Icon from '~/components/Icon.vue'
import CreateNodeDialog from '~/components/nodes/CreateNodeDialog.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useNodeTree } from '~/composables/useNodes'
import type { Node, NodesReorderArgs } from '~/generated'
import { NODE_DEPTH_MAX } from '~/generated/limits'
import { can } from '~/lib/access'
import { useI18n } from '~/plugins/i18n'

/** A node's children in order, which admins add and reorder here. */
const props = defineProps<{ node: Node }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const tree = useNodeTree()

const children = computed(() => tree.index.value.children.get(props.node.id) ?? [])
const writable = computed(() => can(props.node, 'node.write'))
const tooDeep = computed(() => props.node.depth >= NODE_DEPTH_MAX)
const creating = ref(false)

const reorder = useMutation({ mutationFn: (args: NodesReorderArgs) => nodesResource.reorder(args) })
const move = (child: string, position: number) => reorder.mutate({ node: child, position })
const below = (id: string) => tree.index.value.children.get(id)?.length ?? 0
</script>

<template>
  <div class="grid gap-3">
    <ul
      v-if="children.length > 0"
      class="divide-y divide-divider overflow-hidden rounded-panel border border-hairline bg-card"
    >
      <li
        v-for="(child, at) in children"
        :key="child.id"
        class="flex min-h-12 items-center gap-3 px-4 py-2"
      >
        <Icon
          name="lucide:folder"
          size="16"
          class="shrink-0 text-muted"
          aria-hidden="true"
        />
        <RouterLink
          :to="{ name: 'node-settings', params: { nodeId: child.id } }"
          class="min-w-0 flex-1 truncate rounded-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          {{ child.name ?? t('nodes.hidden') }}
        </RouterLink>
        <span class="shrink-0 text-xs text-muted tabular-nums max-sm:hidden">
          {{ t('nodes.childCount', { count: below(child.id) }, below(child.id)) }}
        </span>
        <div
          v-if="writable && children.length > 1"
          class="flex shrink-0 gap-1"
        >
          <Button
            variant="ghost"
            size="icon"
            :disabled="at === 0 || reorder.isPending.value"
            :aria-label="t('nodes.moveUp', { name: child.name ?? '' })"
            @click="move(child.id, at - 1)"
          >
            <Icon name="lucide:arrow-up" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            :disabled="at === children.length - 1 || reorder.isPending.value"
            :aria-label="t('nodes.moveDown', { name: child.name ?? '' })"
            @click="move(child.id, at + 1)"
          >
            <Icon name="lucide:arrow-down" />
          </Button>
        </div>
      </li>
    </ul>
    <p
      v-else
      class="text-sm text-muted"
    >
      {{ writable ? t('nodes.noChildrenWritable') : t('nodes.noChildren') }}
    </p>
    <Alert
      v-if="reorder.error.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(reorder.error.value) }}</p>
    </Alert>
    <div v-if="writable">
      <Button
        variant="fill"
        :disabled="tooDeep"
        @click="creating = true"
      >
        <Icon name="lucide:plus" />
        {{ t('nodes.create.title') }}
      </Button>
      <p
        v-if="tooDeep"
        class="mt-2 text-xs text-muted"
      >
        {{ t('nodes.create.tooDeep', { max: NODE_DEPTH_MAX }) }}
      </p>
    </div>
    <CreateNodeDialog
      v-model:open="creating"
      :parent="node.id"
      :parent-name="node.name"
    />
  </div>
</template>
