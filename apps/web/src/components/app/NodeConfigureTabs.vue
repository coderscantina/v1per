<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { nodeSection } from '~/composables/useNodes'
import type { NodeId } from '~/generated'
import type { IconName } from '~/lib/icons'
import { configureSections, type ConfigureSection } from '~/lib/modes'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * A node's configuration as one row of tabs above the page, in place of a sidebar entry per
 * section. An item's page (one channel, one status page) marks its list's tab.
 */
const props = defineProps<{ node: NodeId }>()

const { t } = useI18n()
const route = useRoute()

const sections: Record<ConfigureSection, { label: MessageKey; icon: IconName }> = {
  'node-monitors': { label: 'nav.monitors', icon: 'lucide:activity' },
  'node-pages': { label: 'nav.pages', icon: 'lucide:globe' },
  'node-components': { label: 'nav.components', icon: 'lucide:box' },
  'node-channels': { label: 'nav.channels', icon: 'lucide:bell' },
  'node-probes': { label: 'nav.probes', icon: 'lucide:radio-tower' },
  'node-maintenance': { label: 'nav.maintenance', icon: 'lucide:wrench' },
  'node-members': { label: 'nav.members', icon: 'lucide:users' },
  'node-settings': { label: 'nav.settings', icon: 'lucide:settings-2' },
}

const current = computed(() => nodeSection(route.name))
const tabs = computed(() =>
  configureSections.map((name) => ({ name, ...sections[name], to: { name, params: { nodeId: props.node } } }))
)
</script>

<template>
  <nav
    :aria-label="t('shell.configure')"
    class="flex shrink-0 scroll-fade-x gap-1 overflow-x-auto border-b border-hairline px-[calc(var(--shell-page-x)-0.625rem)] scrollbar-none"
  >
    <RouterLink
      v-for="tab in tabs"
      :key="tab.name"
      :to="tab.to"
      class="relative flex h-11 shrink-0 items-center gap-[7px] rounded-sm px-3 text-[0.8125rem] font-medium whitespace-nowrap text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
      :class="current === tab.name && 'text-primary after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-accent'"
      :aria-current="current === tab.name ? 'page' : undefined"
    >
      <Icon
        :name="tab.icon"
        size="13"
        aria-hidden="true"
      />
      {{ t(tab.label) }}
    </RouterLink>
  </nav>
</template>
