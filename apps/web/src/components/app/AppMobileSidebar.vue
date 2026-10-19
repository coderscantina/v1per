<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppMobileSidebar.vue. The account
// block is gone: identity and account links live in the header's user menu on every width. The
// node and monitor tree replaces the navigation list.
import AppBrand from '~/components/app/AppBrand.vue'
import AppJumpField from '~/components/app/AppJumpField.vue'
import AppSidebarFooter from '~/components/app/AppSidebarFooter.vue'
import AppTree from '~/components/app/AppTree.vue'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '~/components/ui/sheet'
import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const sidebar = useSidebar()
const route = useRoute()

// A drawer that survives the navigation it triggered covers the page just asked for.
watch(
  () => route.fullPath,
  () => {
    sidebar.mobileOpen.value = false
  }
)

const close = () => {
  sidebar.mobileOpen.value = false
}
</script>

<template>
  <Sheet v-model:open="sidebar.mobileOpen.value">
    <SheetContent
      side="left"
      data-frame
      class="flex w-[min(20rem,85vw)] flex-col gap-0 border-sidebar-border bg-sidebar p-0 pt-safe pb-safe text-sidebar-foreground"
    >
      <SheetTitle class="sr-only">{{ t('nav.label') }}</SheetTitle>
      <SheetDescription class="sr-only">{{ t('nav.drawerDescription') }}</SheetDescription>
      <div class="flex h-shell-header shrink-0 items-center px-3">
        <AppBrand />
      </div>
      <div class="shrink-0 px-3 pb-2">
        <AppJumpField @open="close" />
      </div>
      <div class="flex min-h-0 flex-1 flex-col px-3">
        <AppTree @navigate="close" />
      </div>
      <div class="px-3">
        <AppSidebarFooter @navigate="close" />
      </div>
    </SheetContent>
  </Sheet>
</template>
