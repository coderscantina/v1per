import { useQueryClient } from '@tanstack/vue-query'
import { computed, onScopeDispose, ref, shallowRef, type ComputedRef } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { membersResource } from '~/api/resources/members'
import { monitorsResource } from '~/api/resources/monitors'
import { nodesResource } from '~/api/resources/nodes'
import { useConfirm } from '~/composables/useConfirm'
import { useNodeTree } from '~/composables/useNodes'
import { useVisibleMonitors } from '~/composables/useVisibleMonitors'
import type { NodeId } from '~/generated'
import { NODE_DEPTH_MAX } from '~/generated/limits'
import { moveImpact } from '~/lib/access'
import type { TreeRow } from '~/lib/monitor-tree'
import { pathTo } from '~/lib/node-tree'
import { canDrag, dropTarget, type DragContext, type Dragged, type DropPlan, type DropTarget } from '~/lib/tree-drag'
import { useI18n } from '~/plugins/i18n'

/** Hovering a collapsed row this long while dragging opens it. */
const EXPAND_DELAY_MS = 600

/**
 * Drag and drop for the shell's tree in configure mode: pointer only, the Move dialog stays the
 * keyboard path. `enabled` turns it off, such as while filtering; `expand` opens a row by key.
 */
export function useTreeDrag(options: { enabled: ComputedRef<boolean>; expand: (key: string) => void }) {
  const { t } = useI18n()
  const { confirm } = useConfirm()
  const queryClient = useQueryClient()
  const nodes = useNodeTree()
  const visible = useVisibleMonitors()

  const context = computed<DragContext>(() => ({
    index: nodes.index.value,
    tree: visible.tree.value,
    maxDepth: NODE_DEPTH_MAX,
  }))

  const dragged = shallowRef<Dragged | null>(null)
  const draggedKey = ref<string | null>(null)
  const over = shallowRef<{ key: string; target: DropTarget } | null>(null)
  const busy = ref(false)
  const error = shallowRef<unknown>(null)

  let hover: { key: string; timer: ReturnType<typeof setTimeout> } | null = null
  const stopHover = () => {
    if (hover) clearTimeout(hover.timer)
    hover = null
  }
  onScopeDispose(stopHover)

  const draggable = (row: TreeRow) => options.enabled.value && !busy.value && canDrag(context.value, row)

  const start = (row: TreeRow, event: DragEvent) => {
    if (!draggable(row) || !event.dataTransfer) return
    dragged.value = row.kind === 'node' ? { kind: 'node', id: row.node.id } : { kind: 'monitor', id: row.monitor.id }
    draggedKey.value = row.key
    error.value = null
    event.dataTransfer.effectAllowed = 'move'
    // Firefox starts a drag only with data set.
    event.dataTransfer.setData('text/plain', row.kind === 'node' ? (row.node.name ?? '') : row.monitor.name)
  }

  const end = () => {
    dragged.value = null
    draggedKey.value = null
    over.value = null
    stopHover()
  }

  const move = (row: TreeRow, event: DragEvent) => {
    const source = dragged.value
    if (!source || !(event.currentTarget instanceof HTMLElement)) return
    if (row.expandable && !row.open && hover?.key !== row.key) {
      stopHover()
      hover = { key: row.key, timer: setTimeout(() => options.expand(row.key), EXPAND_DELAY_MS) }
    }
    const box = event.currentTarget.getBoundingClientRect()
    const target = dropTarget(context.value, source, row, (event.clientY - box.top) / box.height)
    if (!target) {
      if (over.value?.key === row.key) over.value = null
      return
    }
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    if (over.value?.key !== row.key || over.value.target.zone !== target.zone) over.value = { key: row.key, target }
  }

  const leave = (row: TreeRow, event: DragEvent) => {
    if (event.currentTarget instanceof Node && event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return
    if (over.value?.key === row.key) over.value = null
    if (hover?.key === row.key) stopHover()
  }

  /** A move to another parent changes who has access; say so unless nobody gains or loses it. */
  const accessAllowed = async (node: NodeId, parent: NodeId) => {
    const read = (id: NodeId) =>
      queryClient.fetchQuery({ queryKey: queryKeys.members(id), queryFn: ({ signal }) => membersResource.list(id, signal), staleTime: 0 })
    const [before, after] = await Promise.all([read(node), read(parent)])
    const impact = moveImpact(before.members, after.members, node)
    if (impact.gains.length + impact.losses.length + impact.changes.length === 0) return true
    const index = nodes.index.value
    return confirm({
      title: t('nodes.move.title', { name: index.byId.get(node)?.name ?? '' }),
      message: t('shell.tree.moveAccess', {
        target: pathTo(index, parent)
          .map((entry) => entry.name ?? '…')
          .join(' / '),
        gains: impact.gains.length,
        losses: impact.losses.length,
        changes: impact.changes.length,
      }),
      confirmLabel: t('nodes.move.confirm'),
    })
  }

  const run = async (plan: DropPlan) => {
    switch (plan.kind) {
      case 'reorder':
        await nodesResource.reorder({ node: plan.node, position: plan.position })
        break
      case 'move':
        if (!(await accessAllowed(plan.node, plan.parent))) return
        await nodesResource.move({ node: plan.node, parent: plan.parent })
        if (plan.position !== null) await nodesResource.reorder({ node: plan.node, position: plan.position })
        break
      case 'monitor':
        // The monitor lists follow their live topics, which carry `monitor.moved`.
        await monitorsResource.move(plan.args)
        return
    }
    await nodes.reload()
  }

  const drop = async (row: TreeRow, event: DragEvent) => {
    event.preventDefault()
    const target = over.value?.key === row.key ? over.value.target : null
    end()
    if (!target || busy.value) return
    busy.value = true
    try {
      await run(target.plan)
    } catch (caught) {
      error.value = caught
    } finally {
      busy.value = false
    }
  }

  /** The indicator a row shows, if the pointer is over it with a valid drop. */
  const zoneOf = (row: TreeRow) => (over.value?.key === row.key ? over.value.target.zone : null)

  return { draggable, draggedKey, zoneOf, busy, error, start, move, leave, drop, end }
}
