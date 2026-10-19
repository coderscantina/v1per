<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useNodeMonitors } from '~/composables/useMonitors'
import type { NodeId } from '~/generated'
import { countPresences, presenceFill, presenceLabels, presences } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

/** How many of a node's monitors are in each status, each count linking to the filtered list. */
const props = defineProps<{ node: NodeId }>()

const { t } = useI18n()
const monitors = useNodeMonitors(() => props.node)
const list = computed(() => monitors.data.value?.monitors ?? null)
const counts = computed(() => (list.value ? countPresences(list.value) : null))
const shown = computed(() => presences.filter((presence) => (counts.value?.[presence] ?? 0) > 0))
</script>

<template>
  <Panel
    id="node-monitors"
    :title="t('monitors.summary.title')"
    :meta="list ? t('monitors.count', { count: list.length }, list.length) : undefined"
  >
    <template #actions>
      <RouterLink
        :to="{ name: 'node-monitors', params: { nodeId: node } }"
        class="inline-flex items-center gap-1 rounded-sm text-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ t('monitors.summary.all') }}
        <Icon
          name="lucide:arrow-right"
          size="14"
          aria-hidden="true"
        />
      </RouterLink>
    </template>
    <Skeleton
      v-if="!counts"
      class="h-12 w-full"
    />
    <p
      v-else-if="shown.length === 0"
      class="text-sm text-muted"
    >
      {{ t('monitors.summary.empty') }}
    </p>
    <ul
      v-else
      class="flex flex-wrap gap-2"
    >
      <li
        v-for="presence in shown"
        :key="presence"
      >
        <RouterLink
          :to="{ name: 'node-monitors', params: { nodeId: node }, query: { status: presence } }"
          class="flex items-center gap-2.5 rounded-control bg-fill px-3.5 py-2 text-sm transition-colors hover:bg-fill-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          :data-status="presence"
        >
          <span
            class="size-2 shrink-0 rounded-full"
            :class="presenceFill[presence]"
            aria-hidden="true"
          />
          <span>
            <span class="text-lg font-semibold text-primary tabular-nums">{{ counts?.[presence] }}</span>{{ ' ' }}<span
              class="text-muted"
              >{{ t(presenceLabels[presence]) }}</span
            >
          </span>
        </RouterLink>
      </li>
    </ul>
  </Panel>
</template>
