<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/pages/workspace/JoinTeamPage.vue. An invitation
// is for one email address: a signed-out invitee without an account creates one here.
import { useMutation, useQuery } from '@tanstack/vue-query'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { invitationsResource } from '~/api/resources/invitations'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { applyLogin, useSession } from '~/composables/useSession'
import type { InvitationsAcceptArgs } from '~/generated'
import { INVITATION_PATH, NAME_MAX_CHARS, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS } from '~/generated/limits'
import { roleLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'

/**
 * Redeems an invitation link. Opening it changes nothing; the code is used only when the
 * person confirms. Unknown, used, revoked and expired codes all answer the same `not_found`.
 */
const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()
const { date } = useDateFormat()
const route = useRoute()
const router = useRouter()
const { account } = useSession()
usePageMeta(() => t('join.title'))

const code = computed(() => (typeof route.query.code === 'string' ? route.query.code.trim() : ''))
const preview = useQuery({
  queryKey: computed(() => queryKeys.invitation(code.value)),
  queryFn: ({ signal }) => invitationsResource.preview(code.value, signal),
  enabled: () => code.value !== '',
  staleTime: 0,
})
const invitation = computed(() => preview.data.value ?? null)
const invalid = computed(
  () => code.value === '' || (preview.error.value instanceof ApiError && preview.error.value.code === 'not_found')
)

const name = ref('')
const password = ref('')
const confirmation = ref('')
const mismatch = computed(() =>
  confirmation.value.length > 0 && password.value !== confirmation.value ? t('account.security.mismatch') : null
)
const signUpReady = computed(
  () =>
    name.value.trim() !== '' &&
    password.value.length >= PASSWORD_MIN_CHARS &&
    password.value.length <= PASSWORD_MAX_CHARS &&
    password.value === confirmation.value
)

const accept = useMutation({
  mutationFn: (args: InvitationsAcceptArgs) => invitationsResource.accept(args),
  onSuccess: async (result) => {
    const target = { name: 'node', params: { nodeId: result.node } } as const
    if (!account.value || result.enrollment_required) {
      applyLogin(
        result.enrollment_required
          ? { status: 'enrollment_required', account: result.account }
          : { status: 'signed_in', account: result.account }
      )
    }
    if (result.enrollment_required) {
      await router.replace({ name: 'enroll', query: { redirect: router.resolve(target).fullPath } })
    } else {
      await router.replace(target)
    }
  },
})

const signInLink = computed(() => ({
  name: 'login',
  query: { redirect: `${INVITATION_PATH}?code=${encodeURIComponent(code.value)}` },
}))
</script>

<template>
  <div class="grid gap-6">
    <h1 class="text-4xl font-semibold tracking-tight text-primary">{{ t('join.title') }}</h1>

    <Alert
      v-if="invalid"
      color="warning"
    >
      <p class="text-sm">{{ t('join.invalid') }}</p>
    </Alert>
    <Alert
      v-else-if="preview.error.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(preview.error.value) }}</p>
    </Alert>
    <div
      v-else-if="!invitation"
      class="grid gap-2"
    >
      <Skeleton class="h-5 w-full" />
      <Skeleton class="h-5 w-2/3" />
    </div>

    <template v-else>
      <div class="grid gap-2">
        <p class="text-lg text-primary">
          {{ t('join.invited', { inviter: invitation.inviter_name, node: invitation.node_name }) }}
        </p>
        <dl class="grid gap-1 text-sm">
          <div class="flex gap-2">
            <dt class="text-muted">{{ t('members.role') }}</dt>
            <dd class="text-primary">{{ t(roleLabels[invitation.role]) }}</dd>
          </div>
          <div class="flex gap-2">
            <dt class="text-muted">{{ t('invitations.email') }}</dt>
            <dd class="min-w-0 break-all text-primary">{{ invitation.email }}</dd>
          </div>
          <div class="flex gap-2">
            <dt class="text-muted">{{ t('invitations.expires') }}</dt>
            <dd class="text-primary">{{ date(invitation.expires_at) }}</dd>
          </div>
        </dl>
      </div>

      <div
        v-if="account"
        class="grid gap-4"
      >
        <p class="text-sm text-muted">{{ t('join.signedInAs', { email: account.email }) }}</p>
        <Alert
          v-if="account.email.toLowerCase() !== invitation.email.toLowerCase()"
          color="warning"
        >
          <p class="text-sm">{{ t('join.otherAccount', { email: invitation.email, account: account.email }) }}</p>
        </Alert>
        <CommandAlert :error="accept.error.value" />
        <Button
          variant="accent"
          size="lg"
          :loading="accept.isPending.value"
          @click="accept.mutate({ code })"
        >
          {{ t('join.accept') }}
        </Button>
      </div>

      <div
        v-else-if="invitation.account_exists"
        class="grid gap-4"
      >
        <p class="text-sm text-muted">{{ t('join.signInFirst', { email: invitation.email }) }}</p>
        <Button
          as-child
          variant="accent"
          size="lg"
        >
          <RouterLink :to="signInLink">{{ t('auth.signIn') }}</RouterLink>
        </Button>
      </div>

      <form
        v-else
        class="grid gap-3.5"
        novalidate
        @submit.prevent="accept.mutate({ code, account: { name, password } })"
      >
        <p class="text-sm text-muted">{{ t('join.createAccount') }}</p>
        <input
          type="email"
          autocomplete="username"
          :value="invitation.email"
          class="sr-only"
          tabindex="-1"
          aria-hidden="true"
          readonly
        >
        <FormField
          id="join-name"
          v-model="name"
          autocomplete="name"
          :label="t('join.name')"
          :maxlength="NAME_MAX_CHARS"
          :error="fieldMessage(accept.error.value, 'account.name')"
          required
        />
        <FormField
          id="join-password"
          v-model="password"
          type="password"
          autocomplete="new-password"
          :label="t('auth.password')"
          :description="t('account.security.passwordHint', { min: PASSWORD_MIN_CHARS })"
          :error="fieldMessage(accept.error.value, 'account.password')"
          required
        />
        <FormField
          id="join-password-confirmation"
          v-model="confirmation"
          type="password"
          autocomplete="new-password"
          :label="t('account.security.confirmPassword')"
          :error="mismatch"
          required
        />
        <CommandAlert
          :error="accept.error.value"
          :shown="['account.name', 'account.password']"
        />
        <Button
          type="submit"
          variant="accent"
          size="lg"
          :loading="accept.isPending.value"
          :disabled="!signUpReady"
        >
          {{ t('join.createAndAccept') }}
        </Button>
      </form>
    </template>
  </div>
</template>
