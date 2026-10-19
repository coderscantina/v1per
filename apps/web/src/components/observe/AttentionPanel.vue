<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import RailSection from '~/components/observe/RailSection.vue'
import type { MonitorSummary, TreeNode } from '~/generated'
import { presenceFill, presenceLabels, presenceOf, presenceRank } from '~/lib/monitor-status'
import { isProblem } from '~/lib/monitor-tree'
import { pathTo, type TreeIndex } from '~/lib/node-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * The monitors in scope that need someone: down first, then pending, each with where it sits
 * below the scope. Each links to its monitor page.
 */
const props = defineProps<{ scope: TreeNode; index: TreeIndex; monitors: MonitorSummary[] }>()

const { t } = useI18n()

const rows = computed(() =>
  props.monitors
    .filter((monitor) => isProblem(presenceOf(monitor)))
    .sort((a, b) => presenceRank(presenceOf(a)) - presenceRank(presenceOf(b)) || a.name.localeCompare(b.name))
    .map((monitor) => ({
      monitor,
      presence: presenceOf(monitor),
      where: pathTo(props.index, monitor.node)
        .slice(props.scope.depth + 1)
        .map((node) => node.name ?? '…')
        .join(' / '),
    }))
)
</script>

<template>
  <RailSection
    id="observe-attention"
    :title="t('observe.attention.title')"
    :meta="String(rows.length)"
  >
    <ul
      v-if="rows.length > 0"
      class="grid gap-2"
    >
      <li
        v-for="row in rows"
        :key="row.monitor.id"
        class="rounded-xl border bg-card px-3 py-[11px]"
        :class="
          row.presence === 'down'
            ? 'border-status-down/30 bg-[linear-gradient(90deg,color-mix(in_oklch,var(--status-down)_9%,transparent),transparent_60%)]'
            : 'border-status-pending/25'
        "
      >
        <div class="flex items-center gap-2">
          <span
            class="relative size-2 shrink-0 rounded-full"
            :class="presenceFill[row.presence]"
            aria-hidden="true"
          >
            <span
              v-if="row.presence === 'down'"
              class="absolute inset-0 rounded-full bg-status-down motion-safe:animate-ping"
            />
          </span>
          <RouterLink
            :to="{ name: 'node-monitor', params: { nodeId: row.monitor.node, monitorId: row.monitor.id } }"
            class="min-w-0 truncate rounded-sm text-[0.8125rem] font-semibold text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            {{ row.monitor.name }}
          </RouterLink>
          <span
            class="ml-auto shrink-0 text-xs"
            :class="row.presence === 'down' ? 'font-medium text-status-down' : 'text-muted'"
          >
            {{ t(presenceLabels[row.presence]) }}
          </span>
        </div>
        <p
          v-if="row.where"
          class="mt-0.5 ml-4 truncate text-xs text-muted"
        >
          {{ row.where }}
        </p>
      </li>
    </ul>
    <p
      v-else
      class="flex items-center gap-2 rounded-xl border border-hairline bg-card px-3 py-2.5 text-[0.8125rem] text-foreground"
    >
      <Icon
        name="lucide:circle-check"
        size="15"
        class="shrink-0 text-status-up"
        aria-hidden="true"
      />
      {{ t('observe.attention.empty') }}
    </p>
  </RailSection>
</template>
