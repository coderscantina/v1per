<script setup lang="ts">
import { useEventListener } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import { useItemSections } from '~/composables/useItemSections'
import { nodeSection, useNodeTree } from '~/composables/useNodes'
import type { NodeId } from '~/generated'
import type { IconName } from '~/lib/icons'
import { configureSections, type ConfigureSection } from '~/lib/modes'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * The bar above a configuration page, led by the folder it belongs to. On a node's pages it
 * holds the node's sections as tabs; an item's page (one channel, one status page) marks its
 * list's tab. A monitor's editor turns it into the monitor's own bar: the folder becomes the
 * way back, and the tabs jump to the editor's sections, marking the one in view.
 */
const props = defineProps<{ node: NodeId }>()

const { t } = useI18n()
const route = useRoute()
const tree = useNodeTree()
const items = useItemSections()

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
const folder = computed(() => tree.index.value.byId.get(props.node)?.name ?? '')
const monitor = computed(() => route.name === 'node-monitor-edit' || route.name === 'node-monitor-new')

/**
 * The editor section in view: the last one whose top has passed the upper part of the canvas,
 * or the last one once the page is scrolled to its end. On a desk the canvas scrolls, on a
 * phone the window does.
 */
const inView = ref<string | null>(null)
const locate = () => {
  const scroller = document.getElementById('main-content')
  const own = scroller !== null && scroller.scrollHeight > scroller.clientHeight
  const top = own ? scroller.getBoundingClientRect().top : 0
  const atEnd = own
    ? scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2
    : window.scrollY + window.innerHeight >= document.documentElement.scrollHeight - 2
  let found = items.value[0]?.id ?? null
  for (const item of items.value) {
    const element = document.getElementById(item.id)
    if (element && element.getBoundingClientRect().top <= top + 120) found = item.id
  }
  inView.value = atEnd ? (items.value.at(-1)?.id ?? found) : found
}
// The canvas renders after this bar, so it is looked up once both are in the page.
const canvas = shallowRef<HTMLElement | null>(null)
onMounted(() => (canvas.value = document.getElementById('main-content')))
useEventListener(canvas, 'scroll', locate, { passive: true })
useEventListener(window, 'scroll', locate, { passive: true })
watch(items, () => void nextTick(locate), { immediate: true })

const jump = (id: string) => {
  document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

const tab =
  'relative flex h-11 shrink-0 items-center gap-[7px] rounded-sm px-3 text-[0.8125rem] font-medium whitespace-nowrap text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring'
const active = 'text-primary after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-accent'
</script>

<template>
  <nav
    :aria-label="monitor ? t('shell.configureMonitor') : t('shell.configure')"
    class="flex shrink-0 scroll-fade-x items-center gap-1 overflow-x-auto border-b border-hairline px-[calc(var(--shell-page-x)-0.625rem)] scrollbar-none"
  >
    <RouterLink
      v-if="monitor"
      :to="{ name: 'node-monitors', params: { nodeId: node } }"
      class="flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-[0.8125rem] font-medium whitespace-nowrap text-muted transition-colors hover:bg-fill hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
    >
      <Icon
        name="lucide:arrow-left"
        size="13"
        aria-hidden="true"
      />
      <Icon
        name="lucide:folder"
        size="13"
        aria-hidden="true"
      />
      {{ folder }}
    </RouterLink>
    <span
      v-else
      class="flex h-8 shrink-0 items-center gap-1.5 px-2.5 text-[0.8125rem] font-semibold whitespace-nowrap text-primary"
    >
      <Icon
        name="lucide:folder"
        size="13"
        class="text-muted"
        aria-hidden="true"
      />
      {{ folder }}
    </span>
    <span
      class="mx-1 h-4 w-px shrink-0 bg-hairline"
      aria-hidden="true"
    />
    <template v-if="monitor">
      <a
        v-for="item in items"
        :key="item.id"
        :href="`#${item.id}`"
        :class="[tab, inView === item.id && active]"
        :aria-current="inView === item.id ? 'location' : undefined"
        @click.prevent="jump(item.id)"
      >
        {{ item.label }}
      </a>
    </template>
    <template v-else>
      <RouterLink
        v-for="entry in tabs"
        :key="entry.name"
        :to="entry.to"
        :class="[tab, current === entry.name && active]"
        :aria-current="current === entry.name ? 'page' : undefined"
      >
        <Icon
          :name="entry.icon"
          size="13"
          aria-hidden="true"
        />
        {{ t(entry.label) }}
      </RouterLink>
    </template>
  </nav>
</template>
