<script setup lang="ts">
import type { PageBoot, PageSnapshot } from '~/generated'

import AccessGate from './AccessGate.vue'
import { linkCode } from './boot'
import { pageClient } from './client'
import { followPage, type Connection } from './live'
import { locale, t } from './messages'
import PageView from './PageView.vue'
import { applyAppearance } from './theme'

/**
 * The status page: the snapshot from the boot JSON at once, then live; or the access screen
 * for a closed page, which turns into the page once the visitor is let in.
 */
const props = defineProps<{ boot: PageBoot | null }>()

const base = props.boot?.base ?? ''
const client = pageClient(base)
const snapshot = shallowRef<PageSnapshot | null>(props.boot?.kind === 'page' ? props.boot.snapshot : null)
const connection = ref<Connection>('connecting')

// A sign-in link opened while already let in: drop the code from the address.
if (props.boot?.kind === 'page' && linkCode(base) !== null) window.history.replaceState(null, '', base || '/')

function show(next: PageSnapshot) {
  snapshot.value = next
  applyAppearance(next.appearance)
  locale.value = next.locale
  document.title = next.appearance.title
}

let stop: (() => void) | null = null
watch(
  () => snapshot.value !== null,
  (shown) => {
    if (!shown || stop || !snapshot.value) return
    stop = followPage({
      client,
      cursor: snapshot.value.cursor,
      apply: show,
      connection: (next) => (connection.value = next),
      reload: () => window.location.reload(),
    })
  },
  { immediate: true }
)
onBeforeUnmount(() => stop?.())
</script>

<template>
  <PageView
    v-if="snapshot"
    :snapshot="snapshot"
    :base="base"
    :connection="connection"
    :client="client"
  />
  <AccessGate
    v-else-if="boot?.kind === 'gate'"
    :gate="boot.gate"
    :base="base"
    :client="client"
    @opened="show"
  />
  <main
    v-else
    id="main-content"
    class="mx-auto flex min-h-svh max-w-xl items-center justify-center px-6"
  >
    <p class="text-foreground">{{ t('error.boot') }}</p>
  </main>
</template>
