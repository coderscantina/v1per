<script setup lang="ts">
import { useStorage } from '@vueuse/core'

import { monitorsResource } from '~/api/resources/monitors'
import { nodesResource } from '~/api/resources/nodes'

import TreeNameRow from '~/components/app/tree/TreeNameRow.vue'
import { useTreeDrag } from '~/components/app/tree/useTreeDrag'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useMode } from '~/composables/useMode'
import { useCurrentNode, useNodeTree } from '~/composables/useNodes'
import { useFormats } from '~/composables/usePreferences'
import { useVisibleMonitors } from '~/composables/useVisibleMonitors'
import type { MonitorSummary, NodeId, TreeNode } from '~/generated'
import { NODE_DEPTH_MAX } from '~/generated/limits'
import { can } from '~/lib/access'
import { formFromMonitor, isCreatable, toInput } from '~/lib/monitor-form'
import { formatUptime, presenceFill, presenceLabels, typeIcons } from '~/lib/monitor-status'
import { monitorKey, nodeKey, treeRows, type TreeRow } from '~/lib/monitor-tree'
import { monitorTarget, nodeTarget } from '~/lib/modes'
import { pathTo } from '~/lib/node-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * Nodes and monitors in one tree, the way Uptime Kuma lists monitors: a status dot on every row,
 * a node showing the worst status below it and how many monitors are down. A row opens its node
 * or monitor in the current mode; in configure mode the dots give way to type icons. Typing
 * filters by name, the alert toggle keeps only what is down or pending. The path to the open
 * page is expanded on arrival; other expanded rows are kept in this browser. In configure mode
 * rows can be dragged to reorder or move them (`useTreeDrag`), a new folder is named in place
 * below a node row or the current node, and a double click or F2 renames a row (`TreeNameRow`).
 */
const emit = defineEmits<{ navigate: [] }>()

const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const route = useRoute()
const nodes = useNodeTree()
const { node: currentNode } = useCurrentNode()
const visible = useVisibleMonitors()
const { mode, section } = useMode()
const { message } = useErrorMessage()

const query = ref('')
const problems = ref(false)
const open = useStorage<string[]>('v1per:tree-open', [])
const openSet = computed(() => new Set(open.value))

const toggle = (key: string) => {
  open.value = openSet.value.has(key) ? open.value.filter((entry) => entry !== key) : [...open.value, key]
}

const param = (name: string) => {
  const value = route.params[name]
  return typeof value === 'string' ? value : null
}

/** The node and group chain of the open page, so it is in view on arrival. */
const routeKeys = computed(() => {
  const node = param('nodeId')
  if (!node) return []
  const keys = pathTo(nodes.index.value, node).map((entry) => nodeKey(entry.id))
  let monitor = visible.tree.value.monitor(param('monitorId') ?? '')
  while (monitor?.group) {
    keys.push(monitorKey(monitor.group))
    monitor = visible.tree.value.monitor(monitor.group)
  }
  return keys
})
watch(
  routeKeys,
  (keys) => {
    const missing = keys.filter((key) => !openSet.value.has(key))
    if (missing.length > 0) open.value = [...open.value, ...missing]
  },
  { immediate: true }
)

const rows = computed(() => treeRows(visible.tree.value, { open: openSet.value, query: query.value, problems: problems.value }))
const filtering = computed(() => query.value.trim() !== '' || problems.value)

const configure = computed(() => mode.value === 'configure')
const target = (row: TreeRow) => {
  if (row.kind === 'node') return nodeTarget(row.node.id, mode.value ?? 'observe', section.value)
  const manage = can(nodes.index.value.byId.get(row.monitor.node), 'monitors.manage')
  return monitorTarget(row.monitor.node, row.monitor.id, mode.value ?? 'observe', manage)
}
const current = (row: TreeRow) =>
  row.kind === 'node' ? param('nodeId') === row.node.id && !param('monitorId') : param('monitorId') === row.monitor.id
const storedName = (row: TreeRow) => (row.kind === 'node' ? (row.node.name ?? '') : row.monitor.name)
/** Saved names until the live tree carries them, so a renamed row never shows its old name. */
const renamed = ref(new Map<string, string>())
const name = (row: TreeRow) => renamed.value.get(row.key) ?? storedName(row)
watch(rows, (list) => {
  const settled = list.filter((row) => renamed.value.get(row.key) === storedName(row))
  if (settled.length === 0) return
  const next = new Map(renamed.value)
  for (const row of settled) next.delete(row.key)
  renamed.value = next
})
const uptime = (monitor: MonitorSummary) => formatUptime(monitor.uptime_24h, numberLocale.value)

/** The parent of the folder being named, if any. */
const draft = ref<NodeId | null>(null)
type DraftRow = { kind: 'draft'; key: string; parent: NodeId; depth: number }

const mayAddFolder = (node: TreeNode | null | undefined): node is TreeNode =>
  !!node && configure.value && can(node, 'node.write') && node.depth < NODE_DEPTH_MAX

/** Opens the parent and its path, clears the filters so it shows, and names a folder below it. */
const addFolder = (parent: NodeId) => {
  query.value = ''
  problems.value = false
  open.value = [...new Set([...open.value, ...pathTo(nodes.index.value, parent).map((entry) => nodeKey(entry.id))])]
  draft.value = parent
}
const closeDraft = (parent: NodeId) => {
  if (draft.value === parent) draft.value = null
}

/** The rows with the draft after the parent's child nodes, where the new folder lands. */
const items = computed<(TreeRow | DraftRow)[]>(() => {
  const list = rows.value
  const parent = draft.value
  const at = parent === null ? -1 : list.findIndex((row) => row.kind === 'node' && row.node.id === parent)
  const host = list[at]
  if (parent === null || !host) return list
  let end = at + 1
  for (let row = list[end]; row && row.depth > host.depth && !(row.depth === host.depth + 1 && row.kind === 'monitor'); row = list[end]) end += 1
  return [...list.slice(0, end), { kind: 'draft', key: `draft:${parent}`, parent, depth: host.depth + 1 }, ...list.slice(end)]
})

const createFolder = async (parent: NodeId, value: string) => {
  await nodesResource.create({ parent, name: value })
  await nodes.reload()
}

/** The key of the row being renamed, if any. */
const renaming = ref<string | null>(null)
const mayRename = (row: TreeRow) =>
  configure.value &&
  (row.kind === 'node'
    ? can(row.node, 'node.write')
    : isCreatable(row.monitor.type) && can(nodes.index.value.byId.get(row.monitor.node), 'monitors.manage'))
const startRename = (row: TreeRow) => {
  if (mayRename(row)) renaming.value = row.key
}

/** A monitor has no rename command: it is saved whole, as the editor would, at the revision read. */
const rename = async (row: TreeRow, value: string) => {
  if (row.kind === 'node') {
    await nodesResource.rename({ node: row.node.id, name: value })
  } else {
    const { monitor } = await monitorsResource.get(row.monitor.id)
    const form = formFromMonitor(monitor)
    if (!form) throw new Error(t('monitors.editor.unsupported'))
    await monitorsResource.update({ ...toInput({ ...form, name: value }), monitor: monitor.id, expected_revision: monitor.revision })
  }
  renamed.value = new Map(renamed.value).set(row.key, value)
}
const rowIcon = (row: TreeRow) => (row.kind === 'node' ? 'lucide:folder' : typeIcons[row.monitor.type])

const drag = useTreeDrag({
  enabled: computed(() => configure.value && !filtering.value),
  expand: (key) => {
    if (!openSet.value.has(key)) open.value = [...open.value, key]
  },
})
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="flex shrink-0 gap-1.5 pb-2">
      <label class="flex h-8 min-w-0 flex-1 items-center gap-[7px] rounded-[9px] border border-hairline bg-surface px-2.5 text-[0.8125rem] text-muted focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring">
        <Icon
          name="lucide:search"
          size="13"
          class="shrink-0"
          aria-hidden="true"
        />
        <input
          v-model="query"
          type="search"
          :aria-label="t('shell.tree.filter')"
          :placeholder="t('shell.tree.filter')"
          class="min-w-0 flex-1 bg-transparent text-primary outline-none placeholder:text-muted"
        >
      </label>
      <button
        type="button"
        class="grid size-8 shrink-0 cursor-pointer place-items-center rounded-[9px] border border-hairline bg-surface text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring aria-pressed:border-status-down/40 aria-pressed:bg-status-down/10 aria-pressed:text-status-down"
        :aria-pressed="problems"
        :aria-label="t('shell.tree.problems')"
        :title="t('shell.tree.problems')"
        @click="problems = !problems"
      >
        <Icon
          name="lucide:triangle-alert"
          size="14"
          aria-hidden="true"
        />
      </button>
      <button
        v-if="mayAddFolder(currentNode)"
        type="button"
        class="grid size-8 shrink-0 cursor-pointer place-items-center rounded-[9px] border border-hairline bg-surface text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        :aria-label="t('shell.tree.newFolderIn', { name: currentNode.name ?? '' })"
        :title="t('shell.tree.newFolderIn', { name: currentNode.name ?? '' })"
        @click="addFolder(currentNode.id)"
      >
        <Icon
          name="lucide:folder-plus"
          size="14"
          aria-hidden="true"
        />
      </button>
    </div>

    <nav
      :aria-label="t('shell.tree.label')"
      class="-mx-3 min-h-0 flex-1 overflow-y-auto px-3 pb-2"
    >
      <Alert
        v-if="drag.error.value"
        color="destructive"
        class="mb-2"
      >
        <div class="flex items-start gap-2">
          <p class="min-w-0 flex-1 text-sm">{{ message(drag.error.value) }}</p>
          <button
            type="button"
            class="grid size-5 shrink-0 cursor-pointer place-items-center rounded-sm text-muted hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
            :aria-label="t('actions.close')"
            @click="drag.error.value = null"
          >
            <Icon
              name="lucide:x"
              size="13"
              aria-hidden="true"
            />
          </button>
        </div>
      </Alert>
      <div
        v-if="visible.loading.value && rows.length === 0"
        class="grid gap-1.5 pt-1"
      >
        <Skeleton
          v-for="at in 6"
          :key="at"
          class="h-6 rounded-md"
          :style="{ marginLeft: `${(at % 3) * 14}px` }"
        />
      </div>
      <ul
        v-else-if="items.length > 0"
        class="grid grid-cols-[minmax(0,1fr)] gap-px"
        :aria-busy="drag.busy.value || undefined"
      >
        <template
          v-for="row in items"
          :key="row.key"
        >
          <TreeNameRow
            v-if="row.kind === 'draft'"
            icon="lucide:folder"
            :label="t('shell.tree.folderName')"
            :depth="row.depth"
            :save="(value) => createFolder(row.parent, value)"
            @done="closeDraft(row.parent)"
          />
          <TreeNameRow
            v-else-if="renaming === row.key"
            :icon="rowIcon(row)"
            :label="t('shell.tree.rename', { name: name(row) })"
            :initial="name(row)"
            :depth="row.depth"
            :save="(value) => rename(row, value)"
            @done="renaming = null"
          />
          <li
            v-else
            class="group/row relative flex h-[30px] items-center rounded-lg transition-colors duration-150 ease-butter hover:bg-fill"
            :class="[
              current(row) && 'bg-nav-active! text-nav-active-foreground before:absolute before:inset-y-[7px] before:-left-3 before:w-[3px] before:rounded-r-[3px] before:bg-accent',
              drag.zoneOf(row) === 'inside' && 'bg-accent/12! ring-1 ring-accent/60 ring-inset',
              drag.draggedKey.value === row.key && 'opacity-50',
            ]"
            :style="{ paddingLeft: `${row.depth * 14}px` }"
            :draggable="drag.draggable(row) ? 'true' : undefined"
            @dragstart="drag.start(row, $event)"
            @dragover="drag.move(row, $event)"
            @dragleave="drag.leave(row, $event)"
            @drop="drag.drop(row, $event)"
            @dragend="drag.end"
          >
            <span
              v-if="drag.zoneOf(row) === 'before' || drag.zoneOf(row) === 'after'"
              class="pointer-events-none absolute right-1 z-10 h-0.5 rounded-full bg-accent"
              :class="drag.zoneOf(row) === 'before' ? '-top-px' : '-bottom-px'"
              :style="{ left: `${row.depth * 14 + 20}px` }"
              aria-hidden="true"
            />
            <button
              v-if="row.expandable && !filtering"
              type="button"
              class="ml-0.5 grid size-4 shrink-0 cursor-pointer place-items-center rounded-sm text-faint hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
              :aria-expanded="row.open"
              :aria-label="t(row.open ? 'shell.tree.collapse' : 'shell.tree.expand', { name: name(row) })"
              @click="toggle(row.key)"
            >
              <Icon
                name="lucide:chevron-right"
                size="12"
                class="transition-transform duration-150"
                :class="row.open && 'rotate-90'"
                aria-hidden="true"
              />
            </button>
            <span
              v-else
              class="ml-0.5 w-4 shrink-0"
            />
            <RouterLink
              :to="target(row)"
              class="flex h-full min-w-0 flex-1 items-center gap-[7px] rounded-lg pr-2 pl-[7px] text-[0.8125rem] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
              :aria-current="current(row) ? 'page' : undefined"
              :aria-keyshortcuts="mayRename(row) ? 'F2' : undefined"
              :draggable="drag.draggable(row) ? 'false' : undefined"
              @click="emit('navigate')"
              @dblclick="startRename(row)"
              @keydown.f2.prevent="startRename(row)"
            >
              <Icon
                v-if="configure"
                :name="rowIcon(row)"
                size="14"
                class="shrink-0 text-muted"
                aria-hidden="true"
              />
              <span
                v-else
                class="relative size-2 shrink-0 rounded-full"
                :class="presenceFill[row.presence]"
                aria-hidden="true"
              >
                <span
                  v-if="row.presence === 'down'"
                  class="absolute inset-0 rounded-full bg-status-down motion-safe:animate-ping"
                />
              </span>
              <span
                class="min-w-0 flex-1 truncate"
                :class="row.kind === 'node' ? 'font-[550] text-primary' : 'text-foreground'"
              >
                {{ name(row) }}
              </span>
              <span class="sr-only">{{ t(presenceLabels[row.presence]) }}</span>
              <template v-if="row.kind === 'node'">
                <span
                  v-if="row.down > 0 && !configure"
                  class="shrink-0 rounded-md bg-destructive-background px-1.5 text-2xs leading-[18px] font-semibold text-destructive-foreground"
                >
                  {{ t('shell.tree.down', { count: row.down }) }}
                </span>
                <template v-else>
                  <span
                    class="shrink-0 text-[0.71875rem] text-muted tabular-nums"
                    aria-hidden="true"
                  >{{ row.total }}</span>
                  <span class="sr-only">{{ t('monitors.count', { count: row.total }, row.total) }}</span>
                </template>
              </template>
              <span
                v-else-if="!configure && uptime(row.monitor)"
                class="shrink-0 text-2xs text-muted tabular-nums"
              >
                {{ uptime(row.monitor) }}
              </span>
            </RouterLink>
            <button
              v-if="row.kind === 'node' && mayAddFolder(row.node)"
              type="button"
              class="mr-1 hidden size-5 shrink-0 cursor-pointer place-items-center rounded-sm text-muted group-focus-within/row:grid group-hover/row:grid hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
              :aria-label="t('shell.tree.newFolderIn', { name: name(row) })"
              :title="t('shell.tree.newFolderIn', { name: name(row) })"
              @click="addFolder(row.node.id)"
            >
              <Icon
                name="lucide:plus"
                size="13"
                aria-hidden="true"
              />
            </button>
          </li>
        </template>
      </ul>
      <p
        v-else
        class="px-2 py-6 text-center text-sm text-muted"
      >
        {{ filtering ? t('shell.tree.noMatch') : t('shell.tree.empty') }}
      </p>
    </nav>
  </div>
</template>
