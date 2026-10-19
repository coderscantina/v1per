<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppHeader.vue. The notification
// bell is dropped; the breadcrumb shows the node path and the mode switch sits in the middle.
import AppBrand from '~/components/app/AppBrand.vue'
import AppBreadcrumb from '~/components/app/AppBreadcrumb.vue'
import AppJumpField from '~/components/app/AppJumpField.vue'
import AppModeSwitch from '~/components/app/AppModeSwitch.vue'
import AppUserMenu from '~/components/app/AppUserMenu.vue'

/**
 * The top bar sits on the frame, above the canvas: where the page is on the left, observe or
 * configure in the middle, the command palette and the user menu at the right. On a phone the
 * tab bar carries the modes and the drawer the palette.
 */
</script>

<template>
  <header
    data-frame
    class="sticky top-0 z-20 grid h-shell-header shrink-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-3 bg-sidebar px-3 pt-safe lg:static lg:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] lg:pr-shell-inset lg:pl-0"
  >
    <div class="flex min-w-0 items-center gap-2">
      <AppBrand
        compact
        class="lg:hidden"
      />
      <AppBreadcrumb />
    </div>
    <AppModeSwitch class="max-lg:hidden" />
    <div class="flex items-center justify-end gap-3">
      <div class="w-[210px] max-md:hidden">
        <AppJumpField bar />
      </div>
      <AppUserMenu />
    </div>
  </header>
</template>
