<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import type { SlackInput } from '~/lib/channel-form'
import { useI18n } from '~/plugins/i18n'

import TemplateHelp from './TemplateHelp.vue'

/** Kuma's Slack options. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<SlackInput>({ required: true })

const { t } = useI18n()
</script>

<template>
  <div class="grid gap-4">
    <SecretInputField
      id="channel-slack-webhook"
      v-model="model.webhookUrl"
      :label="t('channels.slack.webhookUrl')"
      :error="errorFor('settings.webhook_url')"
    />
    <div class="grid gap-4 sm:grid-cols-3">
      <FormField
        id="channel-slack-channel"
        v-model="model.channel"
        autocomplete="off"
        :label="t('channels.slack.channel')"
        :error="errorFor('settings.channel')"
      />
      <FormField
        id="channel-slack-username"
        v-model="model.username"
        autocomplete="off"
        :label="t('channels.slack.username')"
      />
      <FormField
        id="channel-slack-icon"
        v-model="model.iconEmoji"
        autocomplete="off"
        placeholder=":rotating_light:"
        :label="t('channels.slack.iconEmoji')"
      />
    </div>
    <SwitchField
      id="channel-slack-notify"
      v-model="model.channelNotify"
      :label="t('channels.slack.channelNotify')"
    />
    <SwitchField
      id="channel-slack-rich"
      v-model="model.richMessage"
      :label="t('channels.slack.richMessage')"
    />
    <SwitchField
      id="channel-slack-group"
      v-model="model.includeGroupName"
      :label="t('channels.slack.includeGroupName')"
    />
    <SwitchField
      id="channel-slack-use-template"
      v-model="model.useTemplate"
      :label="t('channels.useTemplate')"
    />
    <template v-if="model.useTemplate">
      <TextareaField
        id="channel-slack-template"
        v-model="model.template"
        code
        :rows="4"
        :label="t('channels.template')"
        :error="errorFor('settings.template')"
        described-by="channel-slack-templates"
      />
      <TemplateHelp id="channel-slack-templates" />
    </template>
  </div>
</template>
