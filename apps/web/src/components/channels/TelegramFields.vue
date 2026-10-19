<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import type { TelegramParseMode } from '~/generated'
import { telegramParseModes, type TelegramInput } from '~/lib/channel-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import TemplateHelp from './TemplateHelp.vue'

/** Kuma's Telegram options. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<TelegramInput>({ required: true })

const { t } = useI18n()
const parseLabels: Record<TelegramParseMode, MessageKey> = {
  plain: 'channels.telegram.parseModes.plain',
  html: 'channels.telegram.parseModes.html',
  markdown_v2: 'channels.telegram.parseModes.markdown_v2',
}
const parseOptions = computed(() => telegramParseModes.map((value) => ({ value, label: t(parseLabels[value]) })))
</script>

<template>
  <div class="grid gap-4">
    <SecretInputField
      id="channel-telegram-token"
      v-model="model.botToken"
      :label="t('channels.telegram.botToken')"
      :error="errorFor('settings.bot_token')"
    />
    <div class="grid gap-4 sm:grid-cols-2">
      <FormField
        id="channel-telegram-chat"
        v-model="model.chatId"
        autocomplete="off"
        :label="t('channels.telegram.chatId')"
        :error="errorFor('settings.chat_id')"
        required
      />
      <FormField
        id="channel-telegram-thread"
        v-model="model.messageThreadId"
        autocomplete="off"
        :label="t('channels.telegram.messageThreadId')"
      />
    </div>
    <FormField
      id="channel-telegram-server"
      v-model="model.serverUrl"
      type="url"
      autocomplete="off"
      placeholder="https://api.telegram.org"
      :label="t('channels.telegram.serverUrl')"
      :error="errorFor('settings.server_url')"
    />
    <SwitchField
      id="channel-telegram-silent"
      v-model="model.sendSilently"
      :label="t('channels.telegram.sendSilently')"
    />
    <SwitchField
      id="channel-telegram-protect"
      v-model="model.protectContent"
      :label="t('channels.telegram.protectContent')"
    />
    <SwitchField
      id="channel-telegram-use-template"
      v-model="model.useTemplate"
      :label="t('channels.useTemplate')"
    />
    <template v-if="model.useTemplate">
      <SelectField
        id="channel-telegram-parse"
        v-model="model.parseMode"
        :label="t('channels.telegram.parseMode')"
        :options="parseOptions"
      />
      <TextareaField
        id="channel-telegram-template"
        v-model="model.template"
        code
        :rows="4"
        :label="t('channels.template')"
        :error="errorFor('settings.template')"
        described-by="channel-telegram-templates"
      />
      <TemplateHelp id="channel-telegram-templates" />
    </template>
  </div>
</template>
