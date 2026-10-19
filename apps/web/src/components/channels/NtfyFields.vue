<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import type { NtfyAuth } from '~/generated'
import { ntfyAuths, type NtfyInput } from '~/lib/channel-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import TemplateHelp from './TemplateHelp.vue'

/** Kuma's ntfy options. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<NtfyInput>({ required: true })

const { t } = useI18n()
const authLabels: Record<NtfyAuth, MessageKey> = {
  none: 'channels.ntfy.auths.none',
  username_password: 'channels.ntfy.auths.username_password',
  access_token: 'channels.ntfy.auths.access_token',
}
const authOptions = computed(() => ntfyAuths.map((value) => ({ value, label: t(authLabels[value]) })))
</script>

<template>
  <div class="grid gap-4">
    <div class="grid gap-4 sm:grid-cols-2">
      <FormField
        id="channel-ntfy-server"
        v-model="model.serverUrl"
        type="url"
        autocomplete="off"
        :label="t('channels.ntfy.serverUrl')"
        :error="errorFor('settings.server_url')"
      />
      <FormField
        id="channel-ntfy-topic"
        v-model="model.topic"
        autocomplete="off"
        :label="t('channels.ntfy.topic')"
        :error="errorFor('settings.topic')"
        required
      />
      <FormField
        id="channel-ntfy-priority"
        v-model="model.priority"
        inputmode="numeric"
        :label="t('channels.ntfy.priority')"
        :description="t('channels.ntfy.priorityHint')"
        :error="errorFor('settings.priority')"
      />
      <FormField
        id="channel-ntfy-priority-down"
        v-model="model.priorityDown"
        inputmode="numeric"
        :label="t('channels.ntfy.priorityDown')"
        :description="t('channels.ntfy.priorityHint')"
        :error="errorFor('settings.priority_down')"
      />
    </div>
    <SelectField
      id="channel-ntfy-auth"
      v-model="model.auth"
      :label="t('channels.ntfy.auth')"
      :options="authOptions"
    />
    <div
      v-if="model.auth === 'username_password'"
      class="grid gap-4 sm:grid-cols-2"
    >
      <FormField
        id="channel-ntfy-username"
        v-model="model.username"
        autocomplete="off"
        :label="t('channels.ntfy.username')"
      />
      <SecretInputField
        id="channel-ntfy-password"
        v-model="model.password"
        :label="t('channels.ntfy.password')"
        :error="errorFor('settings.password')"
      />
    </div>
    <SecretInputField
      v-if="model.auth === 'access_token'"
      id="channel-ntfy-token"
      v-model="model.accessToken"
      :label="t('channels.ntfy.accessToken')"
      :error="errorFor('settings.access_token')"
    />
    <div class="grid gap-4 sm:grid-cols-2">
      <FormField
        id="channel-ntfy-icon"
        v-model="model.icon"
        type="url"
        autocomplete="off"
        :label="t('channels.ntfy.icon')"
      />
      <FormField
        id="channel-ntfy-call"
        v-model="model.call"
        autocomplete="off"
        :label="t('channels.ntfy.call')"
      />
    </div>
    <SwitchField
      id="channel-ntfy-use-template"
      v-model="model.useTemplate"
      :label="t('channels.useTemplate')"
    />
    <template v-if="model.useTemplate">
      <FormField
        id="channel-ntfy-title"
        v-model="model.customTitle"
        autocomplete="off"
        :label="t('channels.ntfy.customTitle')"
        :error="errorFor('settings.custom_title')"
      />
      <TextareaField
        id="channel-ntfy-message"
        v-model="model.customMessage"
        code
        :rows="4"
        :label="t('channels.ntfy.customMessage')"
        :error="errorFor('settings.custom_message')"
        described-by="channel-ntfy-templates"
      />
      <TemplateHelp id="channel-ntfy-templates" />
    </template>
  </div>
</template>
