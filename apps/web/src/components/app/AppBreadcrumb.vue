<script setup lang="ts">
import { useMode } from '~/composables/useMode'
import { useCurrentNode } from '~/composables/useNodes'
import { useVisibleMonitors } from '~/composables/useVisibleMonitors'
import { nodeTarget } from '~/lib/modes'
import { useI18n } from '~/plugins/i18n'

/**
 * Where the page sits: the node's path and, on a monitor's pages, the monitor. Each node links
 * to itself in the current mode, so the path steps out without changing what you are doing.
 * The last three levels show on a desk, the last one on a phone; the rest stay readable to
 * assistive technology. Ancestors the caller cannot read show as "…".
 */
const { t } = useI18n()
const route = useRoute()
const { path } = useCurrentNode()
const { mode, section } = useMode()
const visible = useVisibleMonitors()

const SHOWN = 3
const monitor = computed(() => {
  const id = route.params.monitorId
  return typeof id === 'string' ? (visible.tree.value.monitor(id) ?? null) : null
})
const segments = computed(() => {
  const nodes = path.value.map((node) => ({
    key: node.id,
    name: node.name,
    to: node.name === undefined ? null : nodeTarget(node.id, mode.value ?? 'observe', section.value),
  }))
  const all = monitor.value ? [...nodes, { key: `m:${monitor.value.id}`, name: monitor.value.name, to: null }] : nodes
  return all.map((segment, at) => {
    const fromEnd = all.length - 1 - at
    return { ...segment, last: fromEnd === 0, visibility: fromEnd >= SHOWN ? 'sr-only' : fromEnd > 0 ? 'max-lg:sr-only' : '' }
  })
})
const clipped = computed(() => segments.value.length > SHOWN)
</script>

<template>
  <nav
    :aria-label="t('shell.breadcrumb')"
    class="min-w-0"
  >
    <ol class="flex min-w-0 items-center gap-0.5 text-[0.8125rem]">
      <li
        v-if="clipped"
        class="text-muted max-lg:hidden"
        aria-hidden="true"
      >
        … /
      </li>
      <li
        v-for="segment in segments"
        :key="segment.key"
        class="flex min-w-0 items-center gap-1"
        :class="segment.visibility"
      >
        <RouterLink
          v-if="segment.to && !segment.last"
          :to="segment.to"
          class="max-w-40 truncate rounded-md px-1.5 py-1 text-muted transition-colors hover:bg-fill hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          {{ segment.name }}
        </RouterLink>
        <span
          v-else-if="segment.name !== undefined"
          class="truncate px-1.5 font-[550] text-primary"
          :aria-current="segment.last ? 'page' : undefined"
        >
          {{ segment.name }}
        </span>
        <span
          v-else
          class="px-1 text-muted"
        >
          <span aria-hidden="true">…</span>
          <span class="sr-only">{{ t('nodes.hidden') }}</span>
        </span>
        <span
          v-if="!segment.last"
          class="text-muted"
          aria-hidden="true"
        >/</span>
      </li>
    </ol>
  </nav>
</template>
