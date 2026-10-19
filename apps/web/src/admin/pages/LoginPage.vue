<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { sessionResource } from '~/api/resources/session'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { applyLogin } from '~/composables/useSession'
import type { LoginArgs } from '~/generated'
import { safeRedirect } from '~/lib/redirect'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

/**
 * The password step. An account with a second factor continues on the code step; one that a
 * 2FA requirement reaches without a factor continues on enrollment.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const login = useMutation({ mutationFn: (args: LoginArgs) => sessionResource.login(args) })

usePageMeta(() => t('auth.title'))

const email = ref('')
const password = ref('')

onMounted(() => document.getElementById('email')?.focus())

const submit = async () => {
  const step = applyLogin(await login.mutateAsync({ email: email.value, password: password.value }))
  const redirect = safeRedirect(route.query)
  const query = redirect === '/' ? {} : { redirect }
  if (step === 'challenge') await router.push({ name: 'login-verify', query })
  else if (step === 'enroll') await router.replace({ name: 'enroll', query })
  else await router.replace(redirect)
}
</script>

<template>
  <form
    class="grid gap-7"
    @submit.prevent="submit().catch(() => {})"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('auth.title') }}
      <span class="block font-medium text-muted">{{ t('auth.to', { name: runtimeConfig.instance_name }) }}</span>
    </h1>
    <div class="grid gap-3.5">
      <FormField
        id="email"
        v-model="email"
        type="email"
        autocomplete="username"
        :label="t('auth.email')"
        required
      />
      <FormField
        id="password"
        v-model="password"
        type="password"
        autocomplete="current-password"
        :label="t('auth.password')"
        required
      />
      <Alert
        v-if="login.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(login.error.value) }}</p>
      </Alert>
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="login.isPending.value"
      >
        {{ t('auth.signIn') }}
      </Button>
      <p class="text-xs text-muted">{{ t('auth.description') }}</p>
    </div>
  </form>
</template>
