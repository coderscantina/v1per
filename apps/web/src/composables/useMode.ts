import { createSharedComposable } from '@vueuse/core'
import { computed, ref, watch } from 'vue'
import type { RouteLocationRaw } from 'vue-router'
import { useRoute } from 'vue-router'

import { isConfigureSection, modeTarget, type ConfigureSection, type Mode } from '~/lib/modes'

import { nodeSection, useCurrentNode } from './useNodes'

/**
 * The current page's mode, from its route, and where each mode leads from here. Configure
 * returns to the tab last used, so switching back and forth keeps the place. Pages outside a
 * node (account, instance) have no mode and lead to the shell's current node.
 */
export const useMode = createSharedComposable(() => {
  const route = useRoute()
  const { node } = useCurrentNode()
  const last = ref<ConfigureSection>('node-monitors')

  watch(
    () => route.name,
    (name) => {
      const section = nodeSection(name)
      if (route.meta.mode === 'configure' && isConfigureSection(section)) last.value = section
    },
    { immediate: true }
  )

  const mode = computed<Mode | null>(() => route.meta.mode ?? null)
  /** The configuration tab a node opens on in configure mode. */
  const section = computed(() => last.value)
  const param = (name: string) => {
    const value = route.params[name]
    return typeof value === 'string' ? value : null
  }
  const targets = computed(() => {
    const place = { name: String(route.name ?? ''), node: param('nodeId') ?? node.value?.id ?? null, monitor: param('monitorId') }
    return {
      observe: modeTarget(place, 'observe', last.value),
      configure: modeTarget(place, 'configure', last.value),
    } satisfies Record<Mode, RouteLocationRaw | null>
  })

  return { mode, section, targets }
})
