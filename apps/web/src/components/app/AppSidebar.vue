<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppSidebar.vue.
import AppBrand from '~/components/app/AppBrand.vue'
import AppJumpField from '~/components/app/AppJumpField.vue'
import AppSidebarFooter from '~/components/app/AppSidebarFooter.vue'
import AppSidebarResizer from '~/components/app/AppSidebarResizer.vue'
import AppTree from '~/components/app/AppTree.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const sidebar = useSidebar()
const dragging = ref(false)
</script>

<template>
  <aside
    id="app-sidebar"
    data-frame
    class="relative hidden shrink-0 flex-col bg-sidebar text-sidebar-foreground select-none lg:flex"
    :class="!dragging && 'transition-[width] duration-200 ease-butter'"
    :style="{ width: `${sidebar.currentWidth.value}px` }"
  >
    <div
      class="group/brand flex shrink-0 items-center gap-1"
      :class="sidebar.collapsed.value ? 'flex-col px-3 py-2' : 'h-shell-header px-4'"
    >
      <AppBrand :compact="sidebar.collapsed.value" />
      <Button
        v-if="sidebar.collapsible"
        variant="ghost"
        size="icon"
        class="text-muted"
        :class="!sidebar.collapsed.value && 'ml-auto opacity-0 group-hover/brand:opacity-100 focus-visible:opacity-100'"
        :aria-label="t('shell.toggleSidebar')"
        :aria-expanded="!sidebar.collapsed.value"
        aria-controls="app-sidebar"
        @click="sidebar.toggle()"
      >
        <Icon :name="sidebar.collapsed.value ? 'lucide:panel-left-open' : 'lucide:panel-left-close'" />
      </Button>
    </div>

    <div
      v-if="sidebar.collapsed.value"
      class="flex-1 px-3 pb-2"
    >
      <AppJumpField compact />
    </div>
    <div
      v-else
      class="flex min-h-0 flex-1 flex-col px-3 pt-2"
    >
      <AppTree />
    </div>
    <div class="px-3">
      <AppSidebarFooter :compact="sidebar.collapsed.value" />
    </div>

    <AppSidebarResizer
      v-if="sidebar.resizable && !sidebar.collapsed.value"
      v-model:dragging="dragging"
    />
  </aside>
</template>
