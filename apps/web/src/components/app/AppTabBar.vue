<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppTabBar.vue.
import AppModeSwitch from '~/components/app/AppModeSwitch.vue'
import Icon from '~/components/Icon.vue'
import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'

/**
 * The phone's navigation: a Menu tab that opens the drawer with the tree, then the two modes.
 * Hidden from `lg` up, where the sidebar and the top bar take over.
 */
const { t } = useI18n()
const sidebar = useSidebar()
</script>

<template>
  <nav
    :aria-label="t('shell.mobileNavigation')"
    data-frame
    class="fixed inset-x-0 bottom-0 z-30 border-t border-sidebar-border bg-sidebar/95 pb-safe backdrop-blur-md lg:hidden"
  >
    <ul class="grid h-shell-tabbar auto-cols-fr grid-flow-col">
      <li>
        <button
          type="button"
          class="flex h-full w-full flex-col items-center justify-center gap-1 text-2xs font-medium text-sidebar-muted transition-colors duration-200 ease-butter active:text-sidebar-accent-foreground"
          aria-haspopup="dialog"
          :aria-expanded="sidebar.mobileOpen.value"
          @click="sidebar.mobileOpen.value = true"
        >
          <span class="grid h-7 w-12 place-items-center rounded-full">
            <Icon
              name="lucide:panel-left"
              size="20"
              aria-hidden="true"
            />
          </span>
          <span>{{ t('shell.menu') }}</span>
        </button>
      </li>
      <AppModeSwitch stacked />
    </ul>
  </nav>
</template>
