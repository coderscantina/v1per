<script setup lang="ts">
import CommandPalette from '~/components/CommandPalette.vue'
import ConfirmDialog from '~/components/ConfirmDialog.vue'
import KeyboardShortcutsDialog from '~/components/KeyboardShortcutsDialog.vue'
import ReauthDialog from '~/components/ReauthDialog.vue'

import AppLayout from './layouts/AppLayout.vue'
import GuestLayout from './layouts/GuestLayout.vue'

const route = useRoute()

/** Nothing renders before the first navigation resolves, so no shell query runs signed out. */
const ready = computed(() => route.matched.length > 0)
const bare = computed(() => !!(route.meta.guest || route.meta.public || route.meta.enrollment))
const layout = computed(() => (bare.value ? GuestLayout : AppLayout))
const inShell = computed(() => ready.value && !bare.value)

/**
 * A page remounts when its route or one of its params changes, so an editor opened on one
 * monitor never carries its form over to the next. Child routes (a status page's tabs) share
 * their parent's params and keep it mounted.
 */
const pageKey = computed(() => `${route.matched[0]?.path ?? ''}?${JSON.stringify(route.params)}`)
</script>

<template>
  <component
    :is="layout"
    v-if="ready"
  >
    <RouterView v-slot="{ Component }">
      <div
        :key="pageKey"
        class="page-enter flex min-h-full flex-col"
      >
        <component :is="Component" />
      </div>
    </RouterView>
  </component>
  <template v-if="inShell">
    <CommandPalette />
    <KeyboardShortcutsDialog />
    <ReauthDialog />
  </template>
  <ConfirmDialog />
</template>
