<script setup lang="ts">
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import type { ChannelChoice, ChannelId } from '~/generated'
import { providerName } from '~/lib/channels'
import { chosenChannels } from '~/lib/monitor-form'
import { useI18n } from '~/plugins/i18n'

/**
 * The channels a monitor alerts, from those its node may use. Until the person picks, the
 * defaults show as chosen, which is what a new monitor gets; `null` stays until then.
 */
const props = defineProps<{ choices: readonly ChannelChoice[]; error?: string | null }>()
const model = defineModel<ChannelId[] | null>({ required: true })

const { t } = useI18n()

const picked = computed({
  get: () => chosenChannels(model.value, props.choices),
  set: (next: ChannelId[]) => (model.value = props.choices.filter((choice) => next.includes(choice.id)).map((choice) => choice.id)),
})
const options = computed(() =>
  props.choices.map((choice) => ({
    value: choice.id,
    label: choice.name,
    hint: [providerName(choice.provider, t), choice.active ? null : t('channels.inactive')]
      .filter((part) => part !== null)
      .join(' · '),
  }))
)
</script>

<template>
  <CheckboxGroup
      v-if="choices.length > 0"
      v-model="picked"
      :legend="t('monitors.editor.channels')"
      :options="options"
      :error="error"
    />
    <p
      v-else
      class="text-sm text-muted"
    >
      {{ t('monitors.editor.noChannels') }}
    </p>
</template>
