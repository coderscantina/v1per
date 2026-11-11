<script setup lang="ts">
import type { PageAppearance, PageSubscription } from '~/generated'

import { PageError, type PageClient } from './client'
import ComponentChoice from './ComponentChoice.vue'
import LocaleSwitch from './LocaleSwitch.vue'
import { t, type MessageKey } from './messages'
import StatusIcon from './StatusIcon.vue'
import { linkError } from './subscription'

/**
 * What a subscriber's mailed link opens: confirming the subscription, changing what it follows,
 * or leaving. Opening the link changes nothing; each step waits for the visitor's click, so a
 * mail scanner that follows links confirms or removes nobody.
 */
const props = defineProps<{
  appearance: PageAppearance
  base: string
  client: PageClient
  token: string
  /** The link asked to unsubscribe. */
  leaving: boolean
}>()

const subscription = shallowRef<PageSubscription | null>(null)
const chosen = ref<string[]>([])
const loading = ref(true)
const pending = ref(false)
const error = ref<MessageKey | null>(null)
const note = ref<MessageKey | null>(null)
const gone = ref(false)
const leaving = ref(props.leaving)

function show(next: PageSubscription) {
  subscription.value = next
  chosen.value = next.components
}

/** Runs one call with the token, turning a failure into its words. */
async function run(work: () => Promise<void>) {
  if (pending.value) return
  pending.value = true
  error.value = null
  note.value = null
  try {
    await work()
  } catch (failure) {
    error.value = failure instanceof PageError ? linkError(failure.code) : 'error.unknown'
  } finally {
    pending.value = false
    loading.value = false
  }
}

onMounted(() => run(async () => show(await props.client.subscription(props.token))))

const confirm = () =>
  run(async () => {
    show(await props.client.confirmSubscription(props.token))
    note.value = 'subscribe.confirmed'
  })

const save = () =>
  run(async () => {
    show(await props.client.chooseComponents({ token: props.token, components: chosen.value }))
    note.value = 'subscribe.saved'
  })

const leave = () =>
  run(async () => {
    await props.client.unsubscribe(props.token)
    gone.value = true
  })

const lead = computed((): MessageKey | null => {
  if (gone.value) return 'subscribe.gone'
  if (!subscription.value) return null
  if (leaving.value) return 'subscribe.leaveLead'
  return subscription.value.confirmed ? 'subscribe.manageLead' : 'subscribe.confirmLead'
})
</script>

<template>
  <div class="mx-auto flex min-h-svh max-w-md flex-col gap-6 px-4 py-10 sm:justify-center sm:py-16">
    <main
      id="main-content"
      class="flex flex-col gap-6 rounded-panel border border-border bg-card p-6 shadow-soft-sm"
      data-testid="subscription"
    >
      <header class="flex flex-col gap-3">
        <img
          v-if="appearance.logo"
          :src="`${base}${appearance.logo}`"
          alt=""
          class="h-9 w-auto max-w-40 self-start object-contain"
        >
        <p class="inline-flex items-center gap-2 text-sm font-medium text-muted">
          <StatusIcon
            name="bell"
            class="size-4"
          />
          {{ appearance.title }}
        </p>
        <h1 class="text-xl font-semibold tracking-tight text-primary">{{ t('subscribe.viewTitle') }}</h1>
        <p
          v-if="loading"
          role="status"
          class="text-foreground"
        >{{ t('subscribe.working') }}</p>
        <p
          v-else-if="lead"
          class="text-foreground"
        >{{ t(lead, { email: subscription?.email ?? '' }) }}</p>
        <p
          v-if="error"
          role="alert"
          class="rounded-lg bg-destructive-background/40 px-3 py-2 text-sm text-primary"
          data-testid="subscription-error"
        >{{ t(error) }}</p>
        <p
          v-if="note && !gone"
          role="status"
          class="rounded-lg bg-fill px-3 py-2 text-sm text-primary"
          data-testid="subscription-note"
        >{{ t(note) }}</p>
      </header>

      <template v-if="subscription && !gone">
        <div
          v-if="leaving"
          class="flex flex-col gap-2"
        >
          <button
            type="button"
            class="h-shell-control rounded-control bg-accent px-4 font-medium text-accent-foreground hover:bg-accent-hover disabled:opacity-60"
            :disabled="pending"
            data-testid="unsubscribe"
            @click="leave"
          >{{ t('subscribe.leave') }}</button>
          <button
            type="button"
            class="h-shell-control rounded-control border border-border bg-card px-4 font-medium text-primary hover:bg-fill"
            @click="leaving = false"
          >{{ t('subscribe.stay') }}</button>
        </div>
        <button
          v-else-if="!subscription.confirmed"
          type="button"
          class="h-shell-control rounded-control bg-accent px-4 font-medium text-accent-foreground hover:bg-accent-hover disabled:opacity-60"
          :disabled="pending"
          data-testid="continue"
          @click="confirm"
        >{{ t('subscribe.confirm') }}</button>
        <form
          v-else
          class="flex flex-col gap-4"
          @submit.prevent="save"
        >
          <ComponentChoice
            v-if="subscription.available.length > 1"
            v-model="chosen"
            :components="subscription.available"
          />
          <button
            v-if="subscription.available.length > 1"
            type="submit"
            class="h-shell-control rounded-control bg-accent px-4 font-medium text-accent-foreground hover:bg-accent-hover disabled:opacity-60"
            :disabled="pending"
          >{{ t('subscribe.save') }}</button>
          <button
            type="button"
            class="h-shell-control rounded-control border border-border bg-card px-4 font-medium text-primary hover:bg-fill"
            @click="leaving = true"
          >{{ t('subscribe.leave') }}</button>
        </form>
      </template>

      <p class="border-t border-border pt-4 text-sm">
        <a
          :href="base || '/'"
          class="rounded-sm font-medium text-accent-text underline-offset-4 hover:underline"
        >{{ t('subscribe.back') }}</a>
      </p>
    </main>
    <footer class="flex justify-center text-sm text-muted">
      <LocaleSwitch :locales="appearance.locales" />
    </footer>
  </div>
</template>
