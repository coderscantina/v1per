<script setup lang="ts">
import type { PageGate, PageSnapshot } from '~/generated'

import { linkCode } from './boot'
import { PageError, type PageClient } from './client'
import LocaleSwitch from './LocaleSwitch.vue'
import { locale, t, type MessageKey } from './messages'
import StatusIcon from './StatusIcon.vue'

/**
 * The screen of a closed page: the page password, a sign-in link by email, and the instance
 * sign-in for team members, as the page allows. Opened from a sign-in link, it trades the
 * link's code for the page cookie first. Once let in, it hands the page's snapshot up.
 */
const props = defineProps<{ gate: PageGate; base: string; client: PageClient }>()
const emit = defineEmits<{ opened: [snapshot: PageSnapshot] }>()

type Pending = 'password' | 'email' | 'code' | null

const pending = ref<Pending>(null)
const password = ref('')
const passwordError = ref<MessageKey | null>(null)
const email = ref('')
const emailError = ref<MessageKey | null>(null)
const emailSent = ref(false)
const codeError = ref<MessageKey | null>(null)
/** The code of an opened sign-in link, until the visitor continues. Opening the link never uses it. */
const awaiting = ref(linkCode(props.base))

const closed = computed(() => !props.gate.password && !props.gate.email && !props.gate.sign_in_url)

/** The words for a failed access call. */
function message(error: unknown, wrong: MessageKey): MessageKey {
  if (!(error instanceof PageError)) return 'error.unknown'
  switch (error.code) {
    case 'invalid_credentials':
      return wrong
    case 'rate_limited':
      return 'error.rateLimited'
    case 'invalid_arguments':
    case 'malformed_request':
      return wrong === 'error.password' ? 'error.unknown' : 'error.email'
    case 'origin_rejected':
      return 'error.origin'
    case 'network':
      return 'error.network'
    default:
      return 'error.unknown'
  }
}

async function open() {
  emit('opened', await props.client.snapshot())
}

async function submitPassword() {
  if (pending.value || password.value === '') return
  pending.value = 'password'
  passwordError.value = null
  try {
    await props.client.password(password.value)
    await open()
  } catch (error) {
    passwordError.value = message(error, 'error.password')
  } finally {
    pending.value = null
  }
}

async function submitEmail() {
  if (pending.value || email.value.trim() === '') return
  pending.value = 'email'
  emailError.value = null
  try {
    await props.client.requestLink(email.value.trim(), locale.value)
    emailSent.value = true
  } catch (error) {
    emailError.value = message(error, 'error.email')
  } finally {
    pending.value = null
  }
}

/** Trades the link's code for the page cookie, only once the visitor confirms. */
async function redeem() {
  if (pending.value || awaiting.value === null) return
  const code = awaiting.value
  awaiting.value = null
  // The code works once; keep it out of the address, the history and a reload.
  window.history.replaceState(null, '', props.base || '/')
  pending.value = 'code'
  codeError.value = null
  try {
    await props.client.redeem(code)
    await open()
  } catch (error) {
    codeError.value = message(error, 'error.code')
  } finally {
    pending.value = null
  }
}
</script>

<template>
  <div class="mx-auto flex min-h-svh max-w-md flex-col gap-6 px-4 py-10 sm:justify-center sm:py-16">
    <main
      id="main-content"
      class="flex flex-col gap-6 rounded-panel border border-border bg-card p-6 shadow-soft-sm"
      data-testid="access-gate"
    >
      <header class="flex flex-col gap-3">
        <img
          v-if="gate.appearance.logo"
          :src="`${base}${gate.appearance.logo}`"
          alt=""
          class="h-9 w-auto max-w-40 self-start object-contain"
        >
        <p class="inline-flex items-center gap-2 text-sm font-medium text-muted">
          <StatusIcon
            name="lock"
            class="size-4"
          />
          {{ gate.appearance.title }}
        </p>
        <h1 class="text-xl font-semibold tracking-tight text-primary">{{ t('gate.title') }}</h1>
        <p
          v-if="pending === 'code'"
          role="status"
          class="text-foreground"
        >{{ t('gate.redeeming') }}</p>
        <p
          v-else-if="awaiting !== null"
          class="text-foreground"
        >{{ t('gate.confirm') }}</p>
        <p
          v-else-if="closed"
          class="text-foreground"
        >{{ t('gate.closed') }}</p>
        <p
          v-else
          class="text-foreground"
        >{{ t('gate.lead') }}</p>
        <p
          v-if="codeError"
          role="alert"
          class="rounded-lg bg-destructive-background/40 px-3 py-2 text-sm text-primary"
          data-testid="code-error"
        >{{ t(codeError) }}</p>
      </header>

      <button
        v-if="awaiting !== null"
        type="button"
        class="h-shell-control rounded-control bg-accent px-4 font-medium text-accent-foreground hover:bg-accent-hover"
        data-testid="continue"
        @click="redeem"
      >{{ t('gate.continue') }}</button>

      <form
        v-if="gate.password && awaiting === null"
        class="flex flex-col gap-2"
        novalidate
        @submit.prevent="submitPassword"
      >
        <label
          for="page-password"
          class="text-sm font-medium text-primary"
        >{{ t('gate.password') }}</label>
        <input
          id="page-password"
          v-model="password"
          type="password"
          autocomplete="current-password"
          required
          :aria-invalid="passwordError ? 'true' : undefined"
          :aria-describedby="passwordError ? 'page-password-error' : undefined"
          class="h-shell-control rounded-control border border-input-border bg-input px-3 text-primary outline-none focus-visible:border-ring focus-visible:outline-none aria-invalid:border-destructive"
        >
        <p
          v-if="passwordError"
          id="page-password-error"
          role="alert"
          class="text-sm text-destructive-foreground"
        >{{ t(passwordError) }}</p>
        <button
          type="submit"
          class="mt-1 h-shell-control rounded-control bg-accent px-4 font-medium text-accent-foreground hover:bg-accent-hover disabled:opacity-60"
          :disabled="pending !== null"
        >
          {{ pending === 'password' ? t('gate.working') : t('gate.passwordSubmit') }}
        </button>
      </form>

      <p
        v-if="gate.password && gate.email && awaiting === null"
        class="flex items-center gap-3 text-xs text-muted before:h-px before:flex-1 before:bg-border after:h-px after:flex-1 after:bg-border"
      >{{ t('gate.or') }}</p>

      <form
        v-if="gate.email && awaiting === null"
        class="flex flex-col gap-2"
        novalidate
        @submit.prevent="submitEmail"
      >
        <label
          for="page-email"
          class="text-sm font-medium text-primary"
        >{{ t('gate.email') }}</label>
        <p
          id="page-email-lead"
          class="text-sm text-muted"
        >{{ t('gate.emailLead') }}</p>
        <input
          id="page-email"
          v-model="email"
          type="email"
          autocomplete="email"
          inputmode="email"
          required
          :aria-invalid="emailError ? 'true' : undefined"
          :aria-describedby="emailError ? 'page-email-lead page-email-error' : 'page-email-lead'"
          class="h-shell-control rounded-control border border-input-border bg-input px-3 text-primary outline-none focus-visible:border-ring focus-visible:outline-none aria-invalid:border-destructive"
        >
        <p
          v-if="emailError"
          id="page-email-error"
          role="alert"
          class="text-sm text-destructive-foreground"
        >{{ t(emailError) }}</p>
        <p
          v-if="emailSent"
          role="status"
          class="flex items-start gap-2 rounded-lg bg-fill px-3 py-2 text-sm text-primary"
          data-testid="email-sent"
        >
          <StatusIcon
            name="mail"
            class="mt-0.5 size-4 shrink-0"
          />
          {{ t('gate.emailSent') }}
        </p>
        <button
          type="submit"
          class="mt-1 h-shell-control rounded-control border border-border bg-card px-4 font-medium text-primary hover:bg-fill disabled:opacity-60"
          :disabled="pending !== null"
        >
          {{ pending === 'email' ? t('gate.working') : t('gate.emailSubmit') }}
        </button>
      </form>

      <p
        v-if="gate.sign_in_url && awaiting === null"
        class="border-t border-border pt-4 text-sm text-muted"
      >
        {{ t('gate.team') }}
        <a
          :href="gate.sign_in_url"
          class="rounded-sm font-medium text-accent-text underline-offset-4 hover:underline"
        >{{ t('gate.signIn') }}</a>
      </p>
    </main>
    <footer class="flex justify-center text-sm text-muted">
      <LocaleSwitch :locales="gate.appearance.locales" />
    </footer>
  </div>
</template>
