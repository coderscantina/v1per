<script setup lang="ts">
import type { PageBoot, PageSnapshot } from '~/generated'

import AccessGate from './AccessGate.vue'
import { linkCode, subscriptionLink } from './boot'
import { pageClient, pageClientKey } from './client'
import { followPage } from './live'
import { locale, t } from './messages'
import PageView from './PageView.vue'
import SubscriptionView from './SubscriptionView.vue'
import { applyAppearance } from './theme'

/**
 * The status page: the snapshot from the boot JSON at once, then live; or the access screen
 * for a closed page, which turns into the page once the visitor is let in.
 */
const props = defineProps<{ boot: PageBoot | null }>()

const base = props.boot?.base ?? ''
const client = pageClient(base)
provide(pageClientKey, client)
const snapshot = shallowRef<PageSnapshot | null>(props.boot?.kind === 'page' ? props.boot.snapshot : null)
/** A subscriber's mailed link. It opens with or without access to the page. */
const subscription = subscriptionLink(base)
const appearance = props.boot?.kind === 'gate' ? props.boot.gate.appearance : props.boot?.snapshot.appearance

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
      reload: () => window.location.reload(),
    })
  },
  { immediate: true }
)
onBeforeUnmount(() => stop?.())
</script>

<template>
  <SubscriptionView
    v-if="subscription && appearance"
    :appearance="appearance"
    :base="base"
    :client="client"
    :token="subscription.token"
    :leaving="subscription.unsubscribe"
  />
  <PageView
    v-else-if="snapshot"
    :snapshot="snapshot"
    :base="base"
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
