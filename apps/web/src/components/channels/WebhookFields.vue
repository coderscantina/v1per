<script setup lang="ts">
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SelectField from '~/components/SelectField.vue'
import TextareaField from '~/components/TextareaField.vue'
import type { WebhookContentType } from '~/generated'
import { webhookContentTypes, webhookMethods, type WebhookInput } from '~/lib/channel-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import TemplateHelp from './TemplateHelp.vue'

/** Kuma's webhook options: where it posts, the body format and extra headers. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<WebhookInput>({ required: true })

const { t } = useI18n()
const contentLabels: Record<WebhookContentType, MessageKey> = {
  json: 'channels.webhook.contentTypes.json',
  form_data: 'channels.webhook.contentTypes.form_data',
  custom: 'channels.webhook.contentTypes.custom',
}
const methodOptions = webhookMethods.map((value) => ({ value, label: value.toUpperCase() }))
const contentOptions = computed(() => webhookContentTypes.map((value) => ({ value, label: t(contentLabels[value]) })))
</script>

<template>
  <div class="grid gap-4">
    <SecretInputField
      id="channel-webhook-url"
      v-model="model.url"
      :label="t('channels.webhook.url')"
      :error="errorFor('settings.url')"
    />
    <div class="grid gap-4 sm:grid-cols-[10rem_1fr]">
      <SelectField
        id="channel-webhook-method"
        v-model="model.method"
        :label="t('channels.webhook.method')"
        :options="methodOptions"
      />
      <SelectField
        id="channel-webhook-content"
        v-model="model.contentType"
        :label="t('channels.webhook.contentType')"
        :options="contentOptions"
      />
    </div>
    <template v-if="model.contentType === 'custom'">
      <TextareaField
        id="channel-webhook-body"
        v-model="model.customBody"
        code
        :rows="5"
        :label="t('channels.webhook.customBody')"
        :error="errorFor('settings.custom_body')"
        described-by="channel-webhook-templates"
      />
      <TemplateHelp id="channel-webhook-templates" />
    </template>
    <SecretInputField
      id="channel-webhook-headers"
      v-model="model.additionalHeaders"
      :label="t('channels.webhook.headers')"
      :error="errorFor('settings.additional_headers')"
    />
  </div>
</template>
