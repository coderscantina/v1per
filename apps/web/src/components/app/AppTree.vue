<script setup lang="ts">
import { useStorage } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import { Skeleton } from '~/components/ui/skeleton'
import { useMode } from '~/composables/useMode'
import { useNodeTree } from '~/composables/useNodes'
import { useVisibleMonitors } from '~/composables/useVisibleMonitors'
import type { MonitorSummary } from '~/generated'
import { can } from '~/lib/access'
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
 * page is expanded on arrival; other expanded rows are kept in this browser.
 */
const emit = defineEmits<{ navigate: [] }>()

const { t, locale } = useI18n()
const route = useRoute()
const nodes = useNodeTree()
const visible = useVisibleMonitors()
const { mode, section } = useMode()

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
const name = (row: TreeRow) => (row.kind === 'node' ? (row.node.name ?? '') : row.monitor.name)
const uptime = (monitor: MonitorSummary) => formatUptime(monitor.uptime_24h, locale.value)
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
    </div>

    <nav
      :aria-label="t('shell.tree.label')"
      class="-mx-3 min-h-0 flex-1 overflow-y-auto px-3 pb-2"
    >
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
        v-else-if="rows.length > 0"
        class="grid grid-cols-[minmax(0,1fr)] gap-px"
      >
        <li
          v-for="row in rows"
          :key="row.key"
          class="group/row relative flex h-[30px] items-center rounded-lg transition-colors duration-150 ease-butter hover:bg-fill"
          :class="current(row) && 'bg-nav-active! text-nav-active-foreground before:absolute before:inset-y-[7px] before:-left-3 before:w-[3px] before:rounded-r-[3px] before:bg-accent'"
          :style="{ paddingLeft: `${row.depth * 14}px` }"
        >
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
            @click="emit('navigate')"
          >
            <Icon
              v-if="configure"
              :name="row.kind === 'node' ? 'lucide:folder' : typeIcons[row.monitor.type]"
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
        </li>
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
