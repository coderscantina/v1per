<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/layouts/AppLayout.vue.
import AppHeader from '~/components/app/AppHeader.vue'
import AppMobileSidebar from '~/components/app/AppMobileSidebar.vue'
import AppSidebar from '~/components/app/AppSidebar.vue'
import AppTabBar from '~/components/app/AppTabBar.vue'
import NodeConfigureTabs from '~/components/app/NodeConfigureTabs.vue'
import SkipLink from '~/components/app/SkipLink.vue'
import ErrorBoundary from '~/components/ErrorBoundary.vue'
import { useMode } from '~/composables/useMode'
import { useCurrentPageTitle } from '~/composables/usePageMeta'
import { useShortcut } from '~/composables/useShortcuts'
import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const sidebar = useSidebar()
const title = useCurrentPageTitle()
const { mode, targets } = useMode()

/** Configure mode frames the canvas in the accent and puts the node's sections above it. */
const configuring = computed(() => {
  const node = route.params.nodeId
  return mode.value === 'configure' && typeof node === 'string' ? node : null
})

/** Read by the live region, so a page change is announced, not just painted. */
const announcement = ref('')

/** On a desk the page scrolls inside the canvas, which is the main region, so a new page starts it at the top. */
const panel = ref<HTMLElement | null>(null)

/**
 * A page change moves focus to the main region and announces the new title. Only the path
 * counts: a query-only change keeps focus on the control in use.
 * The layout mounts only after the navigation that brought it in (App.vue waits for the first
 * one), so every navigation seen here is a later one and stealing focus on first render cannot
 * happen.
 */
const removeFocusHook = router.afterEach((to, from) => {
  if (to.path === from.path) return
  void nextTick(() => {
    panel.value?.scrollTo({ top: 0 })
    panel.value?.focus({ preventScroll: true })
    announcement.value = title.value
  })
})
onScopeDispose(removeFocusHook)

for (const [keys, to] of [['o', 'observe'], ['c', 'configure']] as const) {
  useShortcut({
    keys,
    description: () => t('shell.modes.shortcut', { mode: t(`shell.modes.${to}`) }),
    handler: () => {
      const target = targets.value[to]
      if (target) void router.push(target)
    },
  })
}

useShortcut({
  keys: 'mod+b',
  description: () => t('shortcuts.toggleSidebar'),
  enabled: () => sidebar.collapsible,
  handler: () => sidebar.toggle(),
})
</script>

<template>
  <div class="flex min-h-svh bg-sidebar lg:h-svh lg:overflow-hidden">
    <SkipLink />
    <AppSidebar class="sticky top-0 h-svh" />
    <AppMobileSidebar />

    <div class="flex min-w-0 flex-1 flex-col lg:h-svh">
      <AppHeader />
      <div
        class="flex min-w-0 flex-1 flex-col bg-surface lg:mr-shell-inset lg:mb-shell-inset lg:min-h-0 lg:overflow-hidden lg:rounded-canvas"
        :class="configuring && 'ring-1 ring-accent/50 ring-inset lg:ring-2'"
      >
        <NodeConfigureTabs
          v-if="configuring"
          :node="configuring"
        />
        <main
          id="main-content"
          ref="panel"
          tabindex="-1"
          class="min-w-0 flex-1 overflow-x-clip px-shell-page-x pt-shell-gutter pb-[calc(var(--shell-gutter)+var(--shell-tabbar-height)+env(safe-area-inset-bottom))] focus:outline-none lg:overflow-y-auto lg:pb-shell-gutter"
        >
          <ErrorBoundary>
            <slot />
          </ErrorBoundary>
        </main>
      </div>
    </div>

    <AppTabBar />

    <span
      aria-live="polite"
      aria-atomic="true"
      class="sr-only"
    >
      {{ announcement }}
    </span>
  </div>
</template>
