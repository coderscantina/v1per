<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import Icon from '~/components/Icon.vue'
import RailSection from '~/components/observe/RailSection.vue'
import { Skeleton } from '~/components/ui/skeleton'
import type { MonitorId, NodeId, PageSummary } from '~/generated'
import { accessModeLabels, type AccessMode } from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * The status pages that show a monitor. A page shows monitors from its node and below, so only
 * pages on the monitor's node and the readable nodes above it can, and only their layouts are
 * read. Read again every minute rather than followed live. Shows nothing without a page.
 */
const props = defineProps<{
  monitor: MonitorId
  /** The monitor's node and the readable nodes above it, highest first. */
  nodes: NodeId[]
}>()

const REFRESH_MS = 60_000
const { t } = useI18n()

const shown = useQuery({
  queryKey: computed(() => ['pages.showing', props.monitor, ...props.nodes] as const),
  queryFn: async ({ signal }) => {
    const [scope] = props.nodes
    if (!scope) return []
    const candidates = new Set(props.nodes)
    const { pages } = await pagesResource.list({ node: scope, subtree: true }, signal)
    const details = await Promise.all(
      pages
        .filter((page) => candidates.has(page.node) && page.components > 0)
        .map(async (page) => ({ page, detail: await pagesResource.get(page.id, signal) }))
    )
    return details
      .filter(({ detail }) =>
        detail.groups.some((group) =>
          group.components.some((component) => component.target.type === 'monitor' && component.target.monitor === props.monitor)
        )
      )
      .map(({ page }) => page)
  },
  enabled: () => props.nodes.length > 0,
  refetchInterval: REFRESH_MS,
  staleTime: REFRESH_MS,
})

const modeNames: Record<PageSummary['access_modes'][number], AccessMode> = { password: 'password', email: 'email', token: 'tokens' }
const access = (page: PageSummary): AccessMode => (page.public ? 'public' : page.access_modes[0] ? modeNames[page.access_modes[0]] : 'team')
</script>

<template>
  <RailSection
    v-if="shown.isPending.value || (shown.data.value?.length ?? 0) > 0"
    id="monitor-pages"
    :title="t('monitors.detail.shownOn')"
    :meta="shown.data.value ? String(shown.data.value.length) : undefined"
  >
    <Skeleton
      v-if="!shown.data.value"
      class="h-12 rounded-control"
    />
    <ul
      v-else
      class="divide-y divide-hairline overflow-hidden rounded-control border border-hairline bg-card"
    >
      <li
        v-for="page in shown.data.value"
        :key="page.id"
      >
        <RouterLink
          :to="{ name: 'node-page', params: { nodeId: page.node, pageId: page.id } }"
          class="flex items-center gap-2.5 px-3 py-2.5 transition-colors hover:bg-fill focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
        >
          <Icon
            name="lucide:globe"
            size="14"
            class="shrink-0 text-muted"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 text-[0.8125rem] break-words text-primary">{{ page.title }}</span>
          <span class="shrink-0 text-[0.8125rem] text-muted">{{ t(accessModeLabels[access(page)]) }}</span>
        </RouterLink>
      </li>
    </ul>
  </RailSection>
</template>
