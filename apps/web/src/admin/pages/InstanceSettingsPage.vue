<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { instanceResource } from '~/api/resources/instance'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import TestSend from '~/components/channels/TestSend.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import PageHeader from '~/components/PageHeader.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TimezoneField from '~/components/TimezoneField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useLiveSnapshot } from '~/composables/useLive'
import { usePageMeta } from '~/composables/usePageMeta'
import type { InstanceSettings, InstanceTestSmtpArgs, InstanceUpdateSettingsArgs, InstanceUpdateSmtpArgs, SmtpSettings } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { smtpSecurities, type SmtpSecurity } from '~/lib/channel-form'
import { newSecret, secretField, type SecretInput } from '~/lib/monitor-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

/**
 * The instance admin's settings: the name and the public URL that invitation links start
 * with, the timezone and certificate warnings of notifications, and the mail server that email
 * channels without their own send through. A value set by an environment variable wins over the
 * stored one and shows read-only.
 */
const { t } = useI18n()
const { message, fieldMessage, fieldProblem } = useErrorMessage()
const { confirm } = useConfirm()
usePageMeta(() => t('instance.title'))

const settings = useLiveSnapshot({
  key: 'instance-settings',
  cache: 'instance.settings',
  load: (_, signal) => instanceResource.settings(signal),
})
const info = useLiveSnapshot({
  key: 'instance-info',
  cache: 'instance.info',
  load: (_, signal) => instanceResource.info(signal),
})

const name = ref('')
const publicUrl = ref('')
watch(
  () => settings.data.value,
  (value) => {
    if (!value) return
    name.value = value.name
    publicUrl.value = value.public_url ?? ''
  },
  { immediate: true }
)

const saved = ref(false)
const update = useMutation({
  mutationFn: (args: InstanceUpdateSettingsArgs) => instanceResource.updateSettings(args),
  onSuccess: (result) => {
    saved.value = true
    runtimeConfig.instance_name = result.name
  },
})

const dirty = computed(() => {
  const value = settings.data.value
  return !!value && (name.value.trim() !== value.name || publicUrl.value.trim() !== (value.public_url ?? ''))
})

const submit = () => {
  saved.value = false
  const url = publicUrl.value.trim()
  update.mutate({ name: name.value, public_url: url === '' ? undefined : url })
}

/** Every update carries the name and public URL in effect, since an absent URL clears it. */
const keep = (value: InstanceSettings): InstanceUpdateSettingsArgs => ({
  name: value.name_fixed ? undefined : value.name,
  public_url: value.public_url_fixed ? undefined : value.public_url,
})

const timezone = ref('')
const expiryDays = ref('')
watch(
  () => settings.data.value,
  (value) => {
    if (!value) return
    timezone.value = value.timezone
    expiryDays.value = value.tls_expiry_notify_days.join(', ')
  },
  { immediate: true }
)
const days = computed(() =>
  expiryDays.value
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '')
)
const daysError = computed(() =>
  days.value.length === 0 ? fieldProblem({ code: 'required' }) : days.value.every((entry) => /^\d+$/.test(entry)) ? null : fieldProblem({ code: 'invalid_format' })
)
const notificationsSaved = ref(false)
const updateNotifications = useMutation({
  mutationFn: (args: InstanceUpdateSettingsArgs) => instanceResource.updateSettings(args),
  onMutate: () => (notificationsSaved.value = false),
  onSuccess: () => (notificationsSaved.value = true),
})
const notificationsDirty = computed(() => {
  const value = settings.data.value
  return !!value && (timezone.value.trim() !== value.timezone || days.value.join(',') !== value.tls_expiry_notify_days.join(','))
})
const submitNotifications = () => {
  const value = settings.data.value
  if (!value || daysError.value) return
  updateNotifications.mutate({ ...keep(value), timezone: timezone.value.trim(), tls_expiry_notify_days: days.value.map(Number) })
}

interface SmtpInput {
  host: string
  port: string
  security: SmtpSecurity
  ignoreStarttls: boolean
  ignoreTlsError: boolean
  username: string
  password: SecretInput
  from: string
}
const smtp = ref<SmtpInput>({
  host: '',
  port: '587',
  security: 'starttls',
  ignoreStarttls: false,
  ignoreTlsError: false,
  username: '',
  password: newSecret(),
  from: '',
})
watch(
  () => settings.data.value?.smtp,
  (value) => {
    if (!value) return
    smtp.value = {
      host: value.host,
      port: String(value.port),
      security: value.secure ? 'tls' : 'starttls',
      ignoreStarttls: value.ignore_starttls ?? false,
      ignoreTlsError: value.ignore_tls_error ?? false,
      username: value.username ?? '',
      password: { set: value.password.set, mode: value.password.set ? 'keep' : 'replace', value: '' },
      from: value.from,
    }
  },
  { immediate: true }
)
const securityLabels: Record<SmtpSecurity, MessageKey> = {
  starttls: 'channels.smtp.securities.starttls',
  tls: 'channels.smtp.securities.tls',
}
const securityOptions = computed(() => smtpSecurities.map((value) => ({ value, label: t(securityLabels[value]) })))
const smtpSubmitted = ref(false)
const smtpErrors = computed<Partial<Record<string, string | null>>>(() => {
  if (!smtpSubmitted.value) return {}
  const required = (value: string) => (value.trim() === '' ? fieldProblem({ code: 'required' }) : null)
  return {
    'smtp.host': required(smtp.value.host),
    'smtp.port': required(smtp.value.port) ?? (/^\d+$/.test(smtp.value.port.trim()) ? null : fieldProblem({ code: 'invalid_format' })),
    'smtp.from': required(smtp.value.from),
  }
})
const smtpError = (key: string) => smtpErrors.value[key] ?? fieldMessage(updateSmtp.error.value, key)
const smtpSaved = ref(false)
const updateSmtp = useMutation({
  mutationFn: (args: InstanceUpdateSmtpArgs) => instanceResource.updateSmtp(args),
  onMutate: () => (smtpSaved.value = false),
  onSuccess: () => {
    smtpSaved.value = true
    smtpSubmitted.value = false
  },
})
/** The mail server as the form has it, or `null` once its problems show. */
const smtpSettings = (): SmtpSettings | null => {
  smtpSubmitted.value = true
  if (Object.values(smtpErrors.value).some(Boolean)) return null
  const value = smtp.value
  const tls = value.security === 'tls'
  return {
    host: value.host.trim(),
    port: Number(value.port),
    secure: tls,
    ignore_starttls: tls ? undefined : value.ignoreStarttls,
    ignore_tls_error: value.ignoreTlsError,
    username: value.username.trim() || undefined,
    password: secretField(value.password),
    from: value.from.trim(),
  }
}
const submitSmtp = () => {
  const server = smtpSettings()
  if (server) updateSmtp.mutate({ smtp: server })
}
const removeSmtp = async () => {
  const ok = await confirm({
    title: t('instance.smtp.removeTitle'),
    message: t('instance.smtp.removeConfirm'),
    confirmLabel: t('instance.smtp.remove'),
    variant: 'destructive',
  })
  if (ok) updateSmtp.mutate({})
}

/** A test mail goes through the server as the form has it, so it can be tried before saving. */
const testTo = ref('')
const testSubmitted = ref(false)
const testToError = computed(() => {
  if (!testSubmitted.value) return null
  const to = testTo.value.trim()
  if (to === '') return fieldProblem({ code: 'required' })
  return /^[^\s@]+@[^\s@]+$/.test(to) ? null : fieldProblem({ code: 'invalid_format' })
})
const testArgs = (): InstanceTestSmtpArgs | null => {
  testSubmitted.value = true
  const server = smtpSettings()
  if (!server || testToError.value) return null
  return { to: testTo.value.trim(), smtp: server }
}
</script>

<template>
  <div>
    <PageHeader
      :title="t('instance.title')"
      :description="info.data.value?.name"
    >
    </PageHeader>

    <Alert
      v-if="settings.error.value && !settings.data.value"
      color="destructive"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ message(settings.error.value) }}</p>
    </Alert>
    <Skeleton
      v-else-if="!settings.data.value"
      class="h-48 w-full max-w-4xl rounded-panel"
    />

    <div
      v-else
      class="max-w-4xl"
    >
      <SettingsPage>
        <SettingsSection
          id="instance-general"
          :title="t('instance.general')"
          :description="t('instance.generalDescription')"
        >
          <form
            id="instance-form"
            class="grid gap-4"
            novalidate
            @submit.prevent="submit"
          >
            <FormField
              id="instance-name"
              v-model="name"
              autocomplete="off"
              :label="t('instance.name')"
              :description="
                settings.data.value.name_fixed ? t('instance.fixed', { variable: 'V1PER_INSTANCE_NAME' }) : undefined
              "
              :maxlength="NAME_MAX_CHARS"
              :error="fieldMessage(update.error.value, 'name')"
              :readonly="settings.data.value.name_fixed"
              required
            />
            <FormField
              id="instance-public-url"
              v-model="publicUrl"
              type="url"
              autocomplete="off"
              :label="t('instance.publicUrl')"
              placeholder="https://status.example.com"
              :description="
                settings.data.value.public_url_fixed ? t('instance.fixed', { variable: 'V1PER_PUBLIC_URL' }) : undefined
              "
              :error="fieldMessage(update.error.value, 'public_url')"
              :readonly="settings.data.value.public_url_fixed"
            />
            <CommandAlert
              :error="update.error.value"
              :shown="['name', 'public_url']"
            />
          </form>
          <template
            v-if="!(settings.data.value.name_fixed && settings.data.value.public_url_fixed)"
            #footer
          >
            <Button
              variant="primary"
              type="submit"
              form="instance-form"
              :loading="update.isPending.value"
              :disabled="!dirty || name.trim() === ''"
            >
              {{ t('actions.save') }}
            </Button>
            <span
              v-if="saved && !dirty"
              role="status"
              class="text-sm text-muted"
            >{{ t('actions.saved') }}</span>
          </template>
        </SettingsSection>

        <SettingsSection
          id="instance-notifications"
          :title="t('instance.notifications.title')"
          :description="t('instance.notifications.description')"
        >
          <form
            id="instance-notifications-form"
            class="grid gap-4"
            novalidate
            @submit.prevent="submitNotifications"
          >
            <TimezoneField
              id="instance-timezone"
              v-model="timezone"
              :label="t('instance.notifications.timezone')"
              :error="fieldMessage(updateNotifications.error.value, 'timezone')"
            />
            <FormField
              id="instance-expiry-days"
              v-model="expiryDays"
              inputmode="numeric"
              autocomplete="off"
              :label="t('instance.notifications.expiryDays')"
              :suffix="t('units.days')"
              :description="t('instance.notifications.expiryDaysHint')"
              :error="(expiryDays !== '' || notificationsDirty ? daysError : null) ?? fieldMessage(updateNotifications.error.value, 'tls_expiry_notify_days')"
            />
            <CommandAlert
              :error="updateNotifications.error.value"
              :shown="['timezone', 'tls_expiry_notify_days']"
            />
          </form>
          <template #footer>
            <Button
              variant="primary"
              type="submit"
              form="instance-notifications-form"
              :loading="updateNotifications.isPending.value"
              :disabled="!notificationsDirty || daysError !== null"
            >
              {{ t('actions.save') }}
            </Button>
            <span
              v-if="notificationsSaved && !notificationsDirty"
              role="status"
              class="text-sm text-muted"
            >{{ t('actions.saved') }}</span>
          </template>
        </SettingsSection>

        <SettingsSection
          id="instance-smtp"
          :title="t('instance.smtp.title')"
          :description="t('instance.smtp.description')"
        >
          <form
            id="instance-smtp-form"
            class="grid gap-4"
            novalidate
            @submit.prevent="submitSmtp"
          >
            <Alert
              v-if="!settings.data.value.smtp"
              icon="lucide:info"
            >
              <p class="text-sm">{{ t('instance.smtp.none') }}</p>
            </Alert>
            <div class="grid gap-4 sm:grid-cols-[1fr_8rem]">
              <FormField
                id="instance-smtp-host"
                v-model="smtp.host"
                autocomplete="off"
                :label="t('channels.smtp.host')"
                :error="smtpError('smtp.host')"
                required
              />
              <FormField
                id="instance-smtp-port"
                v-model="smtp.port"
                inputmode="numeric"
                :label="t('channels.smtp.port')"
                :error="smtpError('smtp.port')"
                required
              />
            </div>
            <SelectField
              id="instance-smtp-security"
              v-model="smtp.security"
              :label="t('channels.smtp.security')"
              :options="securityOptions"
            />
            <SwitchField
              v-if="smtp.security === 'starttls'"
              id="instance-smtp-ignore-starttls"
              v-model="smtp.ignoreStarttls"
              :label="t('channels.smtp.ignoreStarttls')"
            />
            <SwitchField
              id="instance-smtp-ignore-tls"
              v-model="smtp.ignoreTlsError"
              :label="t('channels.smtp.ignoreTlsError')"
            />
            <div class="grid gap-4 sm:grid-cols-2">
              <FormField
                id="instance-smtp-username"
                v-model="smtp.username"
                autocomplete="off"
                :label="t('channels.smtp.username')"
                :error="smtpError('smtp.username')"
              />
              <SecretInputField
                id="instance-smtp-password"
                v-model="smtp.password"
                :label="t('channels.smtp.password')"
                :error="smtpError('smtp.password')"
              />
            </div>
            <FormField
              id="instance-smtp-from"
              v-model="smtp.from"
              autocomplete="off"
              placeholder="V1Per <alerts@example.com>"
              :label="t('channels.smtp.from')"
              :error="smtpError('smtp.from')"
              required
            />
            <CommandAlert
              :error="updateSmtp.error.value"
              :shown="['smtp.host', 'smtp.port', 'smtp.username', 'smtp.password', 'smtp.from']"
            />
          </form>
          <template #footer>
            <Button
              variant="primary"
              type="submit"
              form="instance-smtp-form"
              :loading="updateSmtp.isPending.value"
            >
              {{ t('actions.save') }}
            </Button>
            <Button
              v-if="settings.data.value.smtp"
              variant="ghost"
              :disabled="updateSmtp.isPending.value"
              @click="removeSmtp"
            >
              {{ t('instance.smtp.remove') }}
            </Button>
            <span
              v-if="smtpSaved"
              role="status"
              class="text-sm text-muted"
            >{{ t('actions.saved') }}</span>
          </template>
        </SettingsSection>

        <SettingsSection
          id="instance-smtp-test"
          :title="t('instance.smtp.testTitle')"
          :description="t('instance.smtp.testDescription')"
        >
          <FormField
            id="instance-smtp-test-to"
            v-model="testTo"
            type="email"
            autocomplete="email"
            :label="t('instance.smtp.testTo')"
            :error="testToError"
            required
          />
          <TestSend
            :args="testArgs"
            :send="instanceResource.testSmtp"
            :label="t('instance.smtp.testSend')"
          />
        </SettingsSection>

        <SettingsSection
          id="instance-about"
          :title="t('instance.about')"
        >
          <dl class="grid gap-3 text-sm">
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('instance.version') }}</dt>
              <dd class="text-primary">{{ info.data.value?.version ?? '' }}</dd>
            </div>
          </dl>
        </SettingsSection>
      </SettingsPage>
    </div>
  </div>
</template>
