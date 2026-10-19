<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppHeader.vue. Breadcrumbs and the
// notification bell are replaced by the node picker.
import AppBrand from '~/components/app/AppBrand.vue'
import AppNodePicker from '~/components/app/AppNodePicker.vue'
import AppUserMenu from '~/components/app/AppUserMenu.vue'

/**
 * The top bar sits on the frame, above the canvas: the node picker on the left and the user
 * menu at the far right, in one row at every width.
 */
</script>

<template>
  <header
    data-frame
    class="@container sticky top-0 z-20 flex h-shell-header shrink-0 items-center gap-2 bg-sidebar px-3 pt-safe lg:static lg:pr-shell-inset lg:pl-0"
  >
    <AppBrand
      compact
      class="lg:hidden"
    />
    <AppNodePicker />
    <div class="flex-1" />
    <AppUserMenu />
  </header>
</template>
