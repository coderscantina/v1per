<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { nodesResource } from '~/api/resources/nodes'
import Icon from '~/components/Icon.vue'
import OpenIncidentsPanel from '~/components/incidents/OpenIncidentsPanel.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import MaintenancePanel from '~/components/maintenance/MaintenancePanel.vue'
import MonitorSummaryPanel from '~/components/monitors/MonitorSummaryPanel.vue'
import CreateNodeDialog from '~/components/nodes/CreateNodeDialog.vue'
import NodePath from '~/components/nodes/NodePath.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail, useNodeMembers } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { usePageCount } from '~/composables/usePages'
import type { NodesReorderArgs } from '~/generated'
import { NODE_DEPTH_MAX } from '~/generated/limits'
import { can } from '~/lib/access'
import { roleLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'

/**
 * One node: its monitors by status, open incidents and maintenance on it and below, where it
 * sits, who has access, its status pages, and its children, which admins add and order here.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { routeId, path, tree } = useCurrentNode()
const detail = useNodeDetail(routeId)
const members = useNodeMembers(routeId)
const pageCount = usePageCount(routeId)

const node = computed(() => detail.data.value?.node ?? null)
usePageMeta(() => node.value?.name ?? t('nodes.overview'))

const ancestors = computed(() => path.value.slice(0, -1))
/** A path of placeholders only says nothing, so it is left out. */
const showPath = computed(() => ancestors.value.length === 0 || ancestors.value.some((entry) => entry.name !== undefined))
const children = computed(() => (routeId.value ? (tree.index.value.children.get(routeId.value) ?? []) : []))
const writable = computed(() => can(node.value, 'node.write'))
const tooDeep = computed(() => (node.value?.depth ?? 0) >= NODE_DEPTH_MAX)
const creating = ref(false)

const reorder = useMutation({ mutationFn: (args: NodesReorderArgs) => nodesResource.reorder(args) })
const move = (child: string, position: number) => reorder.mutate({ node: child, position })

const twoFactor = computed(() => {
  if (!node.value) return ''
  if (node.value.two_factor_required) return t('nodes.twoFactor.here')
  if (node.value.two_factor_inherited) return t('nodes.twoFactor.inherited')
  return t('nodes.twoFactor.off')
})
</script>

<template>
  <div>
    <PageHeader
      :title="node?.name ?? t('nodes.overview')"
      :description="node ? t(roleLabels[node.role]) : undefined"
    >
      <template
        v-if="node"
        #freshness
      >
        <LiveIndicator :status="detail.status.value" />
      </template>
    </PageHeader>

    <EmptyState
      v-if="detail.error.value && !node"
      icon="lucide:folder-x"
      tone="error"
      :title="t('nodes.unavailable')"
      :description="message(detail.error.value)"
      class="mx-auto max-w-lg"
    >
      <Button
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'home' }">{{ t('notFound.home') }}</RouterLink>
      </Button>
    </EmptyState>

    <div
      v-else-if="!node"
      class="grid max-w-4xl gap-6"
    >
      <Skeleton class="h-40 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-4xl gap-6"
    >
      <MonitorSummaryPanel :node="node.id" />
      <OpenIncidentsPanel :node="node.id" />
      <MaintenancePanel :node="node.id" />

      <Panel
        id="node-about"
        :title="t('nodes.about')"
      >
        <dl class="grid gap-3 text-sm">
          <div
            v-if="showPath"
            class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6"
          >
            <dt class="text-muted">{{ t('nodes.path') }}</dt>
            <dd class="min-w-0">
              <NodePath
                v-if="ancestors.length > 0"
                :path="ancestors"
              />
              <span
                v-else
                class="text-primary"
              >{{ t('nodes.root') }}</span>
            </dd>
          </div>
          <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
            <dt class="text-muted">{{ t('nodes.yourRole') }}</dt>
            <dd class="text-primary">{{ t(roleLabels[node.role]) }}</dd>
          </div>
          <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
            <dt class="text-muted">{{ t('nodes.members') }}</dt>
            <dd>
              <RouterLink
                :to="{ name: 'node-members', params: { nodeId: node.id } }"
                class="rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              >
                {{
                  members.data.value
                    ? t('nodes.memberCount', { count: members.data.value.members.length }, members.data.value.members.length)
                    : t('nodes.membersLink')
                }}
              </RouterLink>
            </dd>
          </div>
          <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
            <dt class="text-muted">{{ t('nodes.statusPages') }}</dt>
            <dd>
              <RouterLink
                :to="{ name: 'node-pages', params: { nodeId: node.id } }"
                class="rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              >
                {{ pageCount !== null ? t('nodes.pageCount', { count: pageCount }, pageCount) : t('nodes.pagesLink') }}
              </RouterLink>
            </dd>
          </div>
          <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
            <dt class="text-muted">{{ t('nodes.twoFactor.label') }}</dt>
            <dd class="text-primary">{{ twoFactor }}</dd>
          </div>
        </dl>
      </Panel>

      <Panel
        id="node-children"
        :title="t('nodes.children')"
        :meta="String(children.length)"
        flush
      >
        <template
          v-if="writable"
          #actions
        >
          <Button
            variant="fill"
            size="sm"
            :disabled="tooDeep"
            @click="creating = true"
          >
            <Icon name="lucide:plus" />
            {{ t('nodes.create.title') }}
          </Button>
        </template>
        <p
          v-if="writable && tooDeep"
          class="px-4.5 pb-3 text-xs text-muted"
        >
          {{ t('nodes.create.tooDeep', { max: NODE_DEPTH_MAX }) }}
        </p>
        <ul
          v-if="children.length > 0"
          class="divide-y divide-divider border-t border-hairline"
          :aria-labelledby="'node-children'"
        >
          <li
            v-for="(child, at) in children"
            :key="child.id"
            class="flex min-h-12 items-center gap-3 px-4.5 py-2"
          >
            <Icon
              name="lucide:folder"
              size="16"
              class="shrink-0 text-muted"
              aria-hidden="true"
            />
            <RouterLink
              :to="{ name: 'node', params: { nodeId: child.id } }"
              class="min-w-0 flex-1 truncate rounded-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
              {{ child.name }}
            </RouterLink>
            <span class="shrink-0 text-xs text-muted tabular-nums max-sm:hidden">
              {{ t('nodes.childCount', { count: tree.index.value.children.get(child.id)?.length ?? 0 }, tree.index.value.children.get(child.id)?.length ?? 0) }}
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
          class="border-t border-hairline px-4.5 py-8 text-center text-sm text-muted"
        >
          {{ writable ? t('nodes.noChildrenWritable') : t('nodes.noChildren') }}
        </p>
        <Alert
          v-if="reorder.error.value"
          color="destructive"
          class="m-3 w-auto"
        >
          <p class="text-sm">{{ message(reorder.error.value) }}</p>
        </Alert>
      </Panel>
    </div>

    <CreateNodeDialog
      v-if="node"
      v-model:open="creating"
      :parent="node.id"
      :parent-name="node.name"
    />
  </div>
</template>
