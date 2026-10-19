<script setup lang="ts">
import { useMediaQuery, useStorage } from '@vueuse/core'

import ChoiceGroup from '~/components/ChoiceGroup.vue'
import Icon from '~/components/Icon.vue'
import OpenIncidentsPanel from '~/components/incidents/OpenIncidentsPanel.vue'
import MaintenancePanel from '~/components/maintenance/MaintenancePanel.vue'
import AttentionPanel from '~/components/observe/AttentionPanel.vue'
import NodeOrbit from '~/components/observe/NodeOrbit.vue'
import NodeWall from '~/components/observe/NodeWall.vue'
import ScopeStats from '~/components/observe/ScopeStats.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useVisibleMonitors } from '~/composables/useVisibleMonitors'
import { can } from '~/lib/access'
import { countPresences, presenceFill, presenceLabels, type Presence } from '~/lib/monitor-status'
import { itemsOf } from '~/lib/monitor-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's dashboard in observe mode, for the node and everything below it: the orbit or the
 * wall, the status in numbers, what needs attention, open incidents and maintenance. It reads
 * the shell's live monitor lists, so it changes the moment a status does. The view is kept in
 * this browser; until one is picked, a node with more than the orbit holds opens as the wall.
 * The canvas runs edge to edge under a dot grid; on a wide screen it fits the window, the orbit or
 * wall takes the height left under the title, and the summary column scrolls on its own.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { node, path, tree: nodes } = useCurrentNode()
const visible = useVisibleMonitors()
usePageMeta(() => node.value?.name ?? t('nodes.overview'))

type View = 'orbit' | 'wall'
/** Items a node may hold before it opens as the wall: the orbit grows, but a phone runs out of room. */
const ORBIT_ITEMS = { wide: 24, narrow: 12 }
const wide = useMediaQuery('(min-width: 768px)')
const chosen = useStorage<string>('v1per:observe-view', '')
const view = computed<View>({
  get: () => {
    if (chosen.value === 'orbit' || chosen.value === 'wall') return chosen.value
    return node.value && itemsOf(visible.tree.value, node.value.id).length > ORBIT_ITEMS[wide.value ? 'wide' : 'narrow'] ? 'wall' : 'orbit'
  },
  set: (next) => {
    chosen.value = next
  },
})
const views = computed(() => [
  { value: 'orbit' as const, label: t('observe.view.orbit'), icon: 'lucide:orbit' as const },
  { value: 'wall' as const, label: t('observe.view.wall'), icon: 'lucide:layout-grid' as const },
])
const LEGEND = ['up', 'pending', 'down', 'maintenance'] as const satisfies readonly Presence[]
/** Where the node sits, as the small line above its name. */
const trail = computed(() =>
  node.value ? path.value.slice(0, -1).map((entry) => entry.name ?? '…').join(' / ') || t('observe.instance') : ''
)

const monitors = computed(() => (node.value ? visible.tree.value.below(node.value.id) : []))
const counts = computed(() => countPresences(monitors.value))
/** Paused monitors are neither healthy nor a problem. */
const active = computed(() => monitors.value.length - counts.value.paused)
const empty = computed(() => monitors.value.length === 0 && node.value !== null && visible.tree.value.children(node.value.id).length === 0)
</script>

<template>
  <EmptyState
    v-if="!node && (nodes.data.value || nodes.error.value)"
    icon="lucide:folder-x"
    tone="error"
    :title="t('nodes.unavailable')"
    :description="nodes.error.value ? message(nodes.error.value) : undefined"
    class="mx-auto mt-12 max-w-lg"
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
    class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_21rem]"
  >
    <Skeleton class="h-96 w-full rounded-panel" />
    <Skeleton class="h-64 w-full rounded-panel" />
  </div>

  <div
    v-else
    class="-mx-shell-page-x -mt-shell-gutter flex flex-col xl:-mb-shell-gutter xl:min-h-0 xl:flex-[1_1_0px] xl:flex-row"
  >
    <section
      :aria-label="t('observe.scope', { name: node.name ?? '' })"
      class="flex min-w-0 flex-1 flex-col bg-[radial-gradient(color-mix(in_oklch,var(--color-primary)_7%,transparent)_1px,transparent_1px)] bg-size-[18px_18px] px-shell-page-x pt-5 pb-4 xl:min-h-0"
    >
      <header class="flex flex-wrap items-start justify-between gap-x-4 gap-y-3">
        <div class="min-w-0">
          <p class="truncate text-xs text-muted">{{ trail }}</p>
          <h1 class="truncate text-[1.375rem]/[1.875rem] font-[650] text-primary">{{ node.name }}</h1>
          <p
            class="mt-1 text-[0.8125rem] text-foreground"
            aria-live="polite"
          >
            <template v-if="visible.loading.value && monitors.length === 0">{{ t('observe.summary.loading') }}</template>
            <template v-else-if="counts.down > 0">
              <strong class="font-semibold text-status-down">{{ t('observe.summary.down', { count: counts.down }, counts.down) }}</strong>
              <template v-if="counts.pending > 0">, {{ t('observe.summary.pending', { count: counts.pending }, counts.pending) }}</template>
            </template>
            <template v-else-if="counts.pending > 0">{{ t('observe.summary.onlyPending', { count: counts.pending }, counts.pending) }}</template>
            <template v-else-if="active > 0">{{ t('observe.summary.healthy', { count: active }, active) }}</template>
            <template v-else-if="monitors.length > 0">{{ t('observe.summary.paused') }}</template>
            <template v-else>{{ t('observe.summary.empty') }}</template>
          </p>
        </div>
        <div class="flex items-center gap-3">
          <ChoiceGroup
            v-if="!empty"
            v-model="view"
            :label="t('observe.view.label')"
            :options="views"
          />
        </div>
      </header>

      <EmptyState
        v-if="empty && !visible.loading.value"
        icon="lucide:activity"
        :title="t('observe.empty.title')"
        :description="t('observe.empty.description')"
        class="my-auto"
      >
        <Button
          v-if="can(node, 'monitors.manage')"
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-monitor-new', params: { nodeId: node.id } }">
            <Icon name="lucide:plus" />
            {{ t('monitors.create') }}
          </RouterLink>
        </Button>
      </EmptyState>
      <NodeOrbit
        v-else-if="view === 'orbit'"
        :key="node.id"
        :tree="visible.tree.value"
        :node="node"
        class="my-2 h-[min(36rem,70svh)] xl:h-auto xl:min-h-[22rem] xl:flex-1"
      />
      <NodeWall
        v-else
        :key="node.id"
        :tree="visible.tree.value"
        :node="node"
        class="-mx-1 mt-4 px-1 pb-1 xl:min-h-0 xl:flex-1 xl:overflow-y-auto"
      />

      <footer
        v-if="!empty"
        class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted"
      >
        <span
          v-for="presence in LEGEND"
          :key="presence"
          class="flex items-center gap-1.5"
        >
          <span
            class="size-2 rounded-full"
            :class="presenceFill[presence]"
            aria-hidden="true"
          />
          {{ t(presenceLabels[presence]) }}
        </span>
        <span class="ml-auto max-md:hidden">{{ t('observe.legendHint') }}</span>
      </footer>
    </section>

    <aside
      :aria-label="t('observe.rail')"
      class="grid content-start gap-[22px] border-divider px-shell-page-x py-[18px] xl:w-84 xl:shrink-0 xl:overflow-y-auto xl:border-l xl:bg-[color-mix(in_oklch,var(--color-card)_30%,var(--color-surface))] xl:px-[18px] max-xl:border-t"
    >
      <ScopeStats :monitors="monitors" />
      <AttentionPanel
        :scope="node"
        :index="nodes.index.value"
        :monitors="monitors"
      />
      <OpenIncidentsPanel :node="node.id" />
      <MaintenancePanel :node="node.id" />
    </aside>
  </div>
</template>
