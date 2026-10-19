<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/account/PasswordSection.vue. The
// command takes the current password itself, so there is no reauthentication prompt.
import { useMutation } from '@tanstack/vue-query'

import { ApiError } from '~/api'
import { accountResource } from '~/api/resources/account'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { ChangePasswordArgs } from '~/generated'
import { PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const current = ref('')
const password = ref('')
const confirmation = ref('')
const changed = ref(false)

const change = useMutation({
  mutationFn: (args: ChangePasswordArgs) => accountResource.changePassword(args),
  onSuccess: () => {
    current.value = ''
    password.value = ''
    confirmation.value = ''
    changed.value = true
  },
})

/** A wrong current password belongs at its field, not in the general alert. */
const wrongCurrent = computed(() => change.error.value instanceof ApiError && change.error.value.code === 'invalid_credentials')

/** Checked here so a typo is caught while the person still looks at the field. */
const mismatch = computed(() =>
  confirmation.value.length > 0 && password.value !== confirmation.value ? t('account.security.mismatch') : null
)
const submittable = computed(
  () =>
    current.value !== '' &&
    password.value.length >= PASSWORD_MIN_CHARS &&
    password.value.length <= PASSWORD_MAX_CHARS &&
    password.value === confirmation.value
)

const submit = () => {
  changed.value = false
  change.mutate({ current_password: current.value, new_password: password.value })
}
</script>

<template>
  <SettingsSection
    id="password"
    :title="t('account.security.password')"
    :description="t('account.security.passwordDescription')"
  >
    <form
      id="password-form"
      class="grid gap-4"
      novalidate
      @submit.prevent="submit"
    >
      <FormField
        id="current-password"
        v-model="current"
        type="password"
        autocomplete="current-password"
        :label="t('account.security.currentPassword')"
        :error="wrongCurrent ? t('account.security.wrongPassword') : null"
        required
      />
      <FormField
        id="new-password"
        v-model="password"
        type="password"
        autocomplete="new-password"
        :label="t('account.security.newPassword')"
        :description="t('account.security.passwordHint', { min: PASSWORD_MIN_CHARS })"
        :error="fieldMessage(change.error.value, 'new_password')"
        required
      />
      <FormField
        id="new-password-confirmation"
        v-model="confirmation"
        type="password"
        autocomplete="new-password"
        :label="t('account.security.confirmPassword')"
        :error="mismatch"
        required
      />
      <CommandAlert
        :error="wrongCurrent ? null : change.error.value"
        :shown="['current_password', 'new_password']"
      />
    </form>
    <template #footer>
      <Button
        variant="primary"
        type="submit"
        form="password-form"
        :loading="change.isPending.value"
        :disabled="!submittable"
      >
        {{ t('account.security.changePassword') }}
      </Button>
      <span
        v-if="changed"
        role="status"
        class="text-sm text-muted"
      >{{ t('account.security.passwordChanged') }}</span>
    </template>
  </SettingsSection>
</template>
