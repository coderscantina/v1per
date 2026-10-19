<script setup lang="ts">
import type { Locale } from '~/generated'

import { locale, t } from './messages'

/** Links to the page in each enabled language; the server renders the one asked for. */
defineProps<{ locales: Locale[] }>()

const names: Record<Locale, string> = { en: 'English', de: 'Deutsch' }

function href(code: Locale): string {
  const url = new URL(window.location.href)
  url.searchParams.set('locale', code)
  return `${url.pathname}${url.search}`
}
</script>

<template>
  <nav
    v-if="locales.length > 1"
    :aria-label="t('footer.language')"
  >
    <ul class="flex items-center gap-3">
      <li
        v-for="code in locales"
        :key="code"
      >
        <a
          :href="href(code)"
          :lang="code"
          :aria-current="code === locale ? 'true' : undefined"
          class="rounded-sm underline-offset-4 hover:underline aria-[current]:font-medium aria-[current]:text-primary"
        >{{ names[code] }}</a>
      </li>
    </ul>
  </nav>
</template>
