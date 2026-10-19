<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { providePageEditor, usePageDetail } from '~/composables/usePages'
import { can } from '~/lib/access'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * One status page: the links to its settings, layout, access, domains and preview on the left,
 * the open tab on the right. The page is read once here, live, and shared with the tabs.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'pages.manage'))

const pageId = computed(() => (typeof route.params.pageId === 'string' ? route.params.pageId : null))
const live = usePageDetail(pageId)
const page = computed(() => live.data.value?.page ?? null)
providePageEditor({ ...live, manage })
usePageMeta(() => page.value?.title ?? t('pages.title'))

// A link may name another node; the page's own node scopes the editor.
watch(page, (current) => {
  if (current && current.node !== routeId.value) {
    void router.replace({ name: route.name ?? 'node-page', params: { nodeId: current.node, pageId: current.id } })
  }
})

const links = [
  { name: 'node-page', labelKey: 'pages.tabs.settings', icon: 'lucide:settings-2' },
  { name: 'node-page-layout', labelKey: 'pages.tabs.layout', icon: 'lucide:layout-list' },
  { name: 'node-page-access', labelKey: 'pages.tabs.access', icon: 'lucide:lock' },
  { name: 'node-page-hostnames', labelKey: 'pages.tabs.hostnames', icon: 'lucide:globe' },
  { name: 'node-page-preview', labelKey: 'pages.tabs.preview', icon: 'lucide:eye' },
] as const satisfies { name: string; labelKey: MessageKey; icon: string }[]
</script>

<template>
  <div>
    <PageHeader
      :title="page?.title ?? t('pages.title')"
      :description="page?.path"
    >
      <template
        v-if="page"
        #actions
      >
        <Button
          as-child
          variant="fill"
        >
          <a
            :href="page.path"
            target="_blank"
            rel="noopener"
          >
            <Icon name="lucide:external-link" />
            {{ t('pages.open') }}
          </a>
        </Button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="live.error.value && !page"
      icon="lucide:globe"
      tone="error"
      :title="t('pages.unavailable')"
      :description="message(live.error.value)"
      class="mx-auto max-w-lg"
    >
      <Button
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'node-pages', params: { nodeId: routeId } }">{{ t('pages.title') }}</RouterLink>
      </Button>
    </EmptyState>

    <div
      v-else
      class="grid w-full gap-6 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-12"
    >
      <aside class="grid min-w-0 content-start self-start lg:sticky lg:top-0">
        <nav
          :aria-label="t('pages.tabs.label')"
          class="scroll-fade-x flex gap-1 overflow-x-auto lg:grid"
        >
          <RouterLink
            v-for="link in links"
            :key="link.name"
            :to="{ name: link.name, params: { nodeId: routeId, pageId } }"
            class="flex h-shell-nav shrink-0 items-center gap-2.5 rounded-control px-3 text-sm font-medium text-muted transition-colors duration-200 ease-butter hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            exact-active-class="bg-nav-active! text-nav-active-foreground!"
          >
            <Icon
              :name="link.icon"
              size="16"
              class="shrink-0"
              aria-hidden="true"
            />
            {{ t(link.labelKey) }}
          </RouterLink>
        </nav>
      </aside>
      <div class="max-w-4xl min-w-0">
        <div
          v-if="!live.data.value || !node.data.value"
          class="grid gap-6"
        >
          <Skeleton class="h-32 w-full rounded-panel" />
          <Skeleton class="h-56 w-full rounded-panel" />
        </div>
        <RouterView v-else />
      </div>
    </div>
  </div>
</template>
