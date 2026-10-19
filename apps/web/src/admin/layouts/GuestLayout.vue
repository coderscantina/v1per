<script setup lang="ts">
import AppBrand from '~/components/app/AppBrand.vue'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

const { t } = useI18n()

const host = window.location.host

/** A strip of daily bars, as a status page draws them, beside the column. One carries the accent. */
const bars = Array.from({ length: 7 }, (_, index) => (index === 4 ? 'bg-accent/15' : 'bg-fill'))
</script>

<template>
  <div class="relative flex min-h-svh flex-col overflow-hidden bg-surface">
    <header class="px-6 pt-6 sm:px-12 sm:pt-8 lg:px-24">
      <AppBrand class="-ml-2 w-fit text-base" />
    </header>
    <main
      id="main-content"
      tabindex="-1"
      class="relative z-10 flex flex-1 items-center px-6 py-12 focus:outline-none sm:px-12 lg:px-24"
    >
      <div class="w-full max-w-sm">
        <slot />
      </div>
    </main>
    <div
      class="pointer-events-none absolute top-1/2 -right-12 hidden h-[min(60vh,440px)] -translate-y-1/2 items-stretch gap-5 lg:flex"
      aria-hidden="true"
    >
      <span
        v-for="(bar, index) in bars"
        :key="index"
        class="w-12 rounded-[14px]"
        :class="bar"
      />
    </div>
    <footer
      class="relative z-10 flex items-center gap-2 px-6 py-5 text-sm text-muted sm:px-12 lg:px-24"
    >
      <span>{{ host }}</span>
      <template v-if="runtimeConfig.version">
        <span aria-hidden="true">·</span>
        <span>{{ t('shell.version', { version: runtimeConfig.version }) }}</span>
      </template>
    </footer>
  </div>
</template>
