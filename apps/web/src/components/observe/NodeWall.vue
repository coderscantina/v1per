<script setup lang="ts">
import type { MonitorSummary, TreeNode } from '~/generated'
import { presenceFill, presenceLabels, presenceOf } from '~/lib/monitor-status'
import { isProblem, monitorsIn, worstOf, type MonitorTree } from '~/lib/monitor-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * A node as a wall of tiles, one per monitor, grouped by child node: dense enough for a node
 * with hundreds of monitors or a screen on the wall. Down tiles blink, and each group lists its
 * problems under the tiles. The monitors directly in the node form the last group.
 */
const props = defineProps<{ tree: MonitorTree; node: TreeNode }>()

const { t } = useI18n()

interface Group {
  key: string
  name: string
  /** The child node's dashboard; the node's own monitors have none. */
  node: string | null
  monitors: MonitorSummary[]
}

const groups = computed(() => {
  const list: Group[] = props.tree.children(props.node.id).map((child) => ({
    key: child.id,
    name: child.name ?? '',
    node: child.id,
    monitors: props.tree.below(child.id),
  }))
  const direct = props.tree.monitorsOn(props.node.id).flatMap((monitor) => monitorsIn(props.tree, { kind: 'monitor', monitor }))
  if (direct.length > 0) {
    const name = props.node.name ?? ''
    list.push({ key: 'direct', name: list.length > 0 ? t('observe.wall.direct', { name }) : name, node: null, monitors: direct })
  }
  return list.map((group) => ({
    ...group,
    presence: worstOf(group.monitors),
    up: group.monitors.filter((monitor) => presenceOf(monitor) === 'up').length,
    problems: group.monitors.filter((monitor) => isProblem(presenceOf(monitor))),
  }))
})

const monitorLink = (monitor: MonitorSummary) => ({ name: 'node-monitor', params: { nodeId: monitor.node, monitorId: monitor.id } })
</script>

<template>
  <ul class="grid w-full content-start gap-3 sm:grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] motion-safe:animate-in motion-safe:fade-in">
    <li
      v-for="group in groups"
      :key="group.key"
      class="rounded-panel border bg-card p-3"
      :class="group.presence === 'down' ? 'border-status-down/50 shadow-[0_0_32px_-12px_var(--status-down)]' : 'border-hairline'"
    >
      <div class="mb-2.5 flex items-center gap-2">
        <span
          class="size-2 shrink-0 rounded-full"
          :class="presenceFill[group.presence]"
          aria-hidden="true"
        />
        <RouterLink
          v-if="group.node"
          :to="{ name: 'node', params: { nodeId: group.node } }"
          class="min-w-0 flex-1 truncate rounded-sm font-semibold text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          {{ group.name }}
        </RouterLink>
        <span
          v-else
          class="min-w-0 flex-1 truncate font-semibold text-primary"
        >{{ group.name }}</span>
        <span class="shrink-0 text-xs text-muted tabular-nums">
          {{ t('observe.wall.up', { up: group.up, total: group.monitors.length }) }}
        </span>
      </div>
      <ul class="flex flex-wrap gap-1">
        <li
          v-for="monitor in group.monitors"
          :key="monitor.id"
        >
          <RouterLink
            :to="monitorLink(monitor)"
            :title="`${monitor.name} · ${t(presenceLabels[presenceOf(monitor)])}`"
            :aria-label="`${monitor.name}, ${t(presenceLabels[presenceOf(monitor)])}`"
            class="block size-5.5 rounded-[5px] transition-[filter] hover:brightness-125 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            :class="[presenceFill[presenceOf(monitor)], presenceOf(monitor) === 'down' && 'motion-safe:animate-pulse']"
          />
        </li>
      </ul>
      <ul
        v-if="group.problems.length > 0"
        class="mt-2.5 grid gap-1.5"
      >
        <li
          v-for="monitor in group.problems"
          :key="monitor.id"
          class="flex items-center gap-2 text-xs"
        >
          <span
            class="size-1.5 shrink-0 rounded-full"
            :class="presenceFill[presenceOf(monitor)]"
            aria-hidden="true"
          />
          <RouterLink
            :to="monitorLink(monitor)"
            class="min-w-0 flex-1 truncate rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            {{ monitor.name }}
          </RouterLink>
          <span class="shrink-0 text-muted">{{ t(presenceLabels[presenceOf(monitor)]) }}</span>
        </li>
      </ul>
    </li>
  </ul>
</template>
