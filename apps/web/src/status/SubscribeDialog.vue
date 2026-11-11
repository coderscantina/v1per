<script setup lang="ts">
import type { PageSubscriptions } from '~/generated'

import { PageError, type PageClient } from './client'
import ComponentChoice from './ComponentChoice.vue'
import { locale, t, type MessageKey } from './messages'
import StatusIcon from './StatusIcon.vue'
import { subscribeError } from './subscription'

/**
 * The Subscribe button and its dialog: a confirmation mail for an address, optionally for some
 * components only, and the feeds and the maintenance calendar where they open without a token.
 */
const props = defineProps<{
  offer: PageSubscriptions
  /** Component names by ID. */
  names: ReadonlyMap<string, string>
  base: string
  client: PageClient
}>()

const dialog = useTemplateRef('dialog')
const email = ref('')
const chosen = ref<string[]>([])
const pending = ref(false)
const sent = ref(false)
const error = ref<MessageKey | null>(null)

const components = computed(() =>
  props.offer.components.flatMap((id) => {
    const name = props.names.get(id)
    return name === undefined ? [] : [{ id, name }]
  })
)

const feeds = computed(() =>
  props.offer.feeds
    ? ([
        { href: `${props.base}/feed.atom`, label: 'subscribe.atom', icon: 'rss' },
        { href: `${props.base}/feed.rss`, label: 'subscribe.rss', icon: 'rss' },
        { href: `${props.base}/maintenance.ics`, label: 'subscribe.calendar', icon: 'calendar' },
      ] as const)
    : []
)

function open() {
  sent.value = false
  error.value = null
  dialog.value?.showModal()
}

async function submit() {
  if (pending.value || email.value.trim() === '') return
  pending.value = true
  error.value = null
  try {
    await props.client.subscribe({ email: email.value.trim(), components: chosen.value, locale: locale.value })
    sent.value = true
  } catch (failure) {
    error.value = failure instanceof PageError ? subscribeError(failure.code) : 'error.unknown'
  } finally {
    pending.value = false
  }
}
</script>

<template>
  <button
    type="button"
    class="inline-flex h-[34px] items-center gap-1.5 rounded-control border border-border bg-card px-3 text-sm font-medium text-primary hover:bg-fill"
    data-testid="subscribe"
    @click="open"
  >
    <StatusIcon
      name="bell"
      class="size-4"
    />
    {{ t('subscribe.button') }}
  </button>
  <dialog
    ref="dialog"
    aria-labelledby="subscribe-title"
    class="m-auto w-[calc(100vw-2rem)] max-w-md rounded-panel border border-border bg-card p-0 text-foreground shadow-soft-sm backdrop:bg-black/50"
    data-testid="subscribe-dialog"
    @click.self="dialog?.close()"
  >
    <div class="flex flex-col gap-5 p-6">
      <header class="flex items-start justify-between gap-3">
        <div class="flex flex-col gap-1">
          <h2
            id="subscribe-title"
            class="text-lg font-semibold tracking-tight text-primary"
          >{{ t('subscribe.title') }}</h2>
          <p class="text-sm text-muted">{{ t('subscribe.lead') }}</p>
        </div>
        <button
          type="button"
          class="-mt-1 -mr-2 inline-flex size-8 shrink-0 items-center justify-center rounded-control text-muted hover:bg-fill hover:text-primary"
          :aria-label="t('subscribe.close')"
          @click="dialog?.close()"
        >
          <StatusIcon
            name="close"
            class="size-4"
          />
        </button>
      </header>

      <p
        v-if="sent"
        role="status"
        class="flex items-start gap-2 rounded-lg bg-fill px-3 py-2 text-sm text-primary"
        data-testid="subscribe-sent"
      >
        <StatusIcon
          name="mail"
          class="mt-0.5 size-4 shrink-0"
        />
        {{ t('subscribe.sent') }}
      </p>
      <form
        v-else-if="offer.email"
        class="flex flex-col gap-3"
        novalidate
        @submit.prevent="submit"
      >
        <div class="flex flex-col gap-2">
          <label
            for="subscribe-email"
            class="text-sm font-medium text-primary"
          >{{ t('subscribe.email') }}</label>
          <input
            id="subscribe-email"
            v-model="email"
            type="email"
            autocomplete="email"
            inputmode="email"
            required
            autofocus
            :aria-invalid="error ? 'true' : undefined"
            :aria-describedby="error ? 'subscribe-error' : undefined"
            class="h-shell-control rounded-control border border-input-border bg-input px-3 text-primary outline-none focus-visible:border-ring focus-visible:outline-none aria-invalid:border-destructive"
          >
          <p
            v-if="error"
            id="subscribe-error"
            role="alert"
            class="text-sm text-destructive-foreground"
          >{{ t(error) }}</p>
        </div>
        <ComponentChoice
          v-if="components.length > 1"
          v-model="chosen"
          :components="components"
        />
        <button
          type="submit"
          class="h-shell-control rounded-control bg-accent px-4 font-medium text-accent-foreground hover:bg-accent-hover disabled:opacity-60"
          :disabled="pending"
        >
          {{ pending ? t('subscribe.working') : t('subscribe.submit') }}
        </button>
      </form>

      <div
        v-if="feeds.length > 0"
        class="flex flex-col gap-2"
        :class="offer.email ? 'border-t border-border pt-4' : ''"
      >
        <p class="text-sm font-medium text-primary">{{ t('subscribe.feeds') }}</p>
        <ul class="flex flex-wrap gap-x-4 gap-y-2 text-sm">
          <li
            v-for="feed in feeds"
            :key="feed.href"
          >
            <a
              :href="feed.href"
              class="inline-flex items-center gap-1.5 rounded-sm font-medium text-accent-text underline-offset-4 hover:underline"
            >
              <StatusIcon
                :name="feed.icon"
                class="size-4"
              />
              {{ t(feed.label) }}
            </a>
          </li>
        </ul>
      </div>
    </div>
  </dialog>
</template>
