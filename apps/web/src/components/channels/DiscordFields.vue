<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import type { DiscordChannelType, DiscordMessageFormat } from '~/generated'
import { discordChannelTypes, discordFormats, type DiscordInput } from '~/lib/channel-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import TemplateHelp from './TemplateHelp.vue'

/** Kuma's Discord options: the webhook, where it posts, how the message looks. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<DiscordInput>({ required: true })

const { t } = useI18n()
const formatLabels: Record<DiscordMessageFormat, MessageKey> = {
  normal: 'channels.discord.formats.normal',
  minimalist: 'channels.discord.formats.minimalist',
  custom: 'channels.discord.formats.custom',
}
const typeLabels: Record<DiscordChannelType, MessageKey> = {
  channel: 'channels.discord.channelTypes.channel',
  create_new_forum_post: 'channels.discord.channelTypes.create_new_forum_post',
  post_to_thread: 'channels.discord.channelTypes.post_to_thread',
}
const formatOptions = computed(() => discordFormats.map((value) => ({ value, label: t(formatLabels[value]) })))
const typeOptions = computed(() => discordChannelTypes.map((value) => ({ value, label: t(typeLabels[value]) })))
</script>

<template>
  <div class="grid gap-4">
    <SecretInputField
      id="channel-discord-webhook"
      v-model="model.webhookUrl"
      :label="t('channels.discord.webhookUrl')"
      :error="errorFor('settings.webhook_url')"
    />
    <FormField
      id="channel-discord-username"
      v-model="model.username"
      autocomplete="off"
      placeholder="Uptime Kuma"
      :label="t('channels.discord.username')"
      :error="errorFor('settings.username')"
    />
    <SelectField
      id="channel-discord-type"
      v-model="model.channelType"
      :label="t('channels.discord.channelType')"
      :options="typeOptions"
    />
    <FormField
      v-if="model.channelType === 'post_to_thread'"
      id="channel-discord-thread"
      v-model="model.threadId"
      autocomplete="off"
      :label="t('channels.discord.threadId')"
      :error="errorFor('settings.thread_id')"
    />
    <FormField
      v-if="model.channelType === 'create_new_forum_post'"
      id="channel-discord-post"
      v-model="model.postName"
      autocomplete="off"
      :label="t('channels.discord.postName')"
      :error="errorFor('settings.post_name')"
    />
    <SelectField
      id="channel-discord-format"
      v-model="model.messageFormat"
      :label="t('channels.discord.messageFormat')"
      :options="formatOptions"
    />
    <FormField
      v-if="model.messageFormat === 'normal'"
      id="channel-discord-prefix"
      v-model="model.prefixMessage"
      autocomplete="off"
      :label="t('channels.discord.prefixMessage')"
      :description="t('channels.discord.prefixMessageHint')"
      :error="errorFor('settings.prefix_message')"
    />
    <template v-if="model.messageFormat === 'custom'">
      <TextareaField
        id="channel-discord-template"
        v-model="model.messageTemplate"
        code
        :rows="5"
        :label="t('channels.discord.messageTemplate')"
        :error="errorFor('settings.message_template')"
        described-by="channel-discord-templates"
      />
      <TemplateHelp id="channel-discord-templates" />
    </template>
    <SwitchField
      id="channel-discord-disable-url"
      v-model="model.disableUrl"
      :label="t('channels.discord.disableUrl')"
    />
    <SwitchField
      id="channel-discord-silent"
      v-model="model.suppressNotifications"
      :label="t('channels.discord.suppressNotifications')"
    />
  </div>
</template>
