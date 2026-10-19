<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { membersResource } from '~/api/resources/members'
import { nodesResource } from '~/api/resources/nodes'
import CommandAlert from '~/components/CommandAlert.vue'
import NodeTreeList from '~/components/nodes/NodeTreeList.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useNodeTree } from '~/composables/useNodes'
import type { Node, NodesMoveArgs, TreeNode } from '~/generated'
import { NODE_DEPTH_MAX } from '~/generated/limits'
import { moveImpact } from '~/lib/access'
import { moveRefusal, pathTo, type MoveRefusal } from '~/lib/node-tree'
import { roleLabels } from '~/lib/roles'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * Moves a node in two steps: pick a new parent among valid targets, then see who gains or
 * loses access before confirming. Access comes from grants on ancestors, so the preview
 * compares the node's member list with the new parent's.
 */
const props = defineProps<{ node: Node }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { message } = useErrorMessage()
const tree = useNodeTree()

const target = ref<TreeNode | null>(null)

const refusals: Record<MoveRefusal, MessageKey> = {
  hidden: 'nodes.move.refusal.hidden',
  inside: 'nodes.move.refusal.inside',
  current: 'nodes.move.refusal.current',
  capability: 'nodes.move.refusal.capability',
  depth: 'nodes.move.refusal.depth',
}
const refuse = (candidate: TreeNode) => {
  const reason = moveRefusal(tree.index.value, props.node.id, candidate, NODE_DEPTH_MAX)
  return reason ? t(refusals[reason], { max: NODE_DEPTH_MAX }) : null
}

const nodeMembers = useQuery({
  queryKey: computed(() => queryKeys.members(props.node.id)),
  queryFn: ({ signal }) => membersResource.list(props.node.id, signal),
  enabled: () => target.value !== null,
  staleTime: 0,
})
const parentMembers = useQuery({
  queryKey: computed(() => queryKeys.members(target.value?.id ?? '')),
  queryFn: ({ signal }) => membersResource.list(target.value?.id ?? '', signal),
  enabled: () => target.value !== null,
  staleTime: 0,
})

const impact = computed(() => {
  const before = nodeMembers.data.value?.members
  const after = parentMembers.data.value?.members
  return before && after ? moveImpact(before, after, props.node.id) : null
})
const unchanged = computed(
  () => impact.value && impact.value.gains.length + impact.value.losses.length + impact.value.changes.length === 0
)
const destination = computed(() =>
  target.value
    ? pathTo(tree.index.value, target.value.id)
        .map((entry) => entry.name ?? '…')
        .join(' / ')
    : ''
)

const move = useMutation({
  mutationFn: (args: NodesMoveArgs) => nodesResource.move(args),
  onSuccess: () => {
    open.value = false
  },
})

watch(open, (value) => {
  if (value) return
  target.value = null
  move.reset()
})

const back = () => {
  target.value = null
  move.reset()
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-lg">
      <DialogHeaderCombined
        :title="t('nodes.move.title', { name: node.name })"
        :description="target ? t('nodes.move.reviewDescription', { target: destination }) : t('nodes.move.pickDescription')"
      />
      <div
        v-if="!target"
        class="overflow-hidden rounded-lg border border-hairline"
      >
        <NodeTreeList
          :index="tree.index.value"
          :label="t('nodes.move.targets')"
          :refuse="refuse"
          @select="(picked) => (target = picked)"
        />
      </div>
      <div
        v-else
        class="grid gap-4"
      >
        <Alert
          v-if="nodeMembers.error.value || parentMembers.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(nodeMembers.error.value ?? parentMembers.error.value) }}</p>
        </Alert>
        <div
          v-else-if="!impact"
          class="grid gap-2"
        >
          <Skeleton class="h-5 w-2/3" />
          <Skeleton class="h-5 w-1/2" />
        </div>
        <p
          v-else-if="unchanged"
          class="text-sm text-muted"
        >
          {{ t('nodes.move.unchanged') }}
        </p>
        <div
          v-else
          class="grid gap-4 text-sm"
        >
          <section
            v-if="impact.gains.length > 0"
            class="grid gap-1.5"
          >
            <h3 class="font-medium text-primary">{{ t('nodes.move.gains', { count: impact.gains.length }) }}</h3>
            <ul class="grid gap-1">
              <li
                v-for="entry in impact.gains"
                :key="entry.member.user"
                class="flex justify-between gap-3"
              >
                <span class="truncate text-primary">{{ entry.member.name }}</span>
                <span class="shrink-0 text-muted">{{ t(roleLabels[entry.role]) }}</span>
              </li>
            </ul>
          </section>
          <section
            v-if="impact.losses.length > 0"
            class="grid gap-1.5"
          >
            <h3 class="font-medium text-destructive">{{ t('nodes.move.losses', { count: impact.losses.length }) }}</h3>
            <ul class="grid gap-1">
              <li
                v-for="member in impact.losses"
                :key="member.user"
                class="flex justify-between gap-3"
              >
                <span class="truncate text-primary">{{ member.name }}</span>
                <span class="shrink-0 text-muted">{{ t(roleLabels[member.role]) }}</span>
              </li>
            </ul>
          </section>
          <section
            v-if="impact.changes.length > 0"
            class="grid gap-1.5"
          >
            <h3 class="font-medium text-primary">{{ t('nodes.move.changes', { count: impact.changes.length }) }}</h3>
            <ul class="grid gap-1">
              <li
                v-for="entry in impact.changes"
                :key="entry.member.user"
                class="flex justify-between gap-3"
              >
                <span class="truncate text-primary">{{ entry.member.name }}</span>
                <span class="shrink-0 text-muted">{{ t(roleLabels[entry.from]) }} → {{ t(roleLabels[entry.to]) }}</span>
              </li>
            </ul>
          </section>
          <p class="text-xs text-muted">{{ t('nodes.move.belowHint') }}</p>
        </div>
        <CommandAlert :error="move.error.value" />
        <div class="flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            @click="back"
          >
            {{ t('actions.back') }}
          </Button>
          <Button
            variant="primary"
            :loading="move.isPending.value"
            :disabled="!impact"
            @click="move.mutate({ node: node.id, parent: target.id })"
          >
            {{ t('nodes.move.confirm') }}
          </Button>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
