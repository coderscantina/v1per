<script setup lang="ts" generic="Args">
import { useMutation } from '@tanstack/vue-query'

import ChoiceGroup from '~/components/ChoiceGroup.vue'
import Icon from '~/components/Icon.vue'
import SelectField from '~/components/SelectField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useDelivery } from '~/composables/useChannels'
import type { TestOptions } from '~/api/resources/channels'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useVisibleMonitors } from '~/composables/useVisibleMonitors'
import type { ChannelTestQueued, DeliveryId, NodeId, TestSample } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Kuma's Test button: queues one message through `send`, such as `channels.test` with the form
 * as it stands, and shows how it went. `args` is `null` while the form cannot be sent. With
 * `node` the sender may also pick a down or up alert, optionally filled from a monitor on the
 * node or below, to check templates.
 */
const props = defineProps<{
  args: () => Args | null
  send: (args: Args, options?: TestOptions) => Promise<ChannelTestQueued>
  label?: string
  node?: NodeId | null
}>()

const { t } = useI18n()
const { message } = useErrorMessage()
const queued = ref<DeliveryId | null>(null)
const EXAMPLE = 'example'
const kind = ref<'message' | TestSample>('message')
const monitor = ref<string>(EXAMPLE)
const { tree } = useVisibleMonitors()
const kinds = computed(() => [
  { value: 'message' as const, label: t('channels.test.kinds.message') },
  { value: 'monitor_down' as const, label: t('channels.test.kinds.down') },
  { value: 'monitor_up' as const, label: t('channels.test.kinds.up') },
])
const monitors = computed(() => [
  { value: EXAMPLE, label: t('channels.test.exampleMonitor') },
  ...(props.node ? tree.value.below(props.node) : []).map((entry) => ({ value: entry.id, label: entry.name })),
])
const options = (): TestOptions =>
  kind.value === 'message' ? {} : { sample: kind.value, monitor: monitor.value === EXAMPLE ? undefined : monitor.value }
const test = useMutation({
  mutationFn: (args: Args) => props.send(args, options()),
  onMutate: () => (queued.value = null),
  onSuccess: ({ delivery }) => (queued.value = delivery),
})
const delivery = useDelivery(queued)
const result = computed(() => delivery.data.value?.delivery ?? null)
const sending = computed(() => test.isPending.value || (queued.value !== null && (!result.value || result.value.state === 'pending')))
const invalid = ref(false)

const send = () => {
  const args = props.args()
  invalid.value = args === null
  if (args) test.mutate(args)
}
</script>

<template>
  <div class="grid gap-3">
    <ChoiceGroup
      v-if="node"
      v-model="kind"
      :label="t('channels.test.kind')"
      :options="kinds"
      class="justify-self-start"
    />
    <SelectField
      v-if="node && kind !== 'message'"
      id="channel-test-monitor"
      v-model="monitor"
      :label="t('channels.test.monitor')"
      :options="monitors"
    />
    <Button
      type="button"
      variant="fill"
      class="justify-self-start"
      :loading="sending"
      @click="send"
    >
      <Icon name="lucide:send" />
      {{ label ?? t('channels.test.send') }}
    </Button>
    <div
      aria-live="polite"
      class="empty:hidden"
    >
      <Alert
        v-if="invalid"
        color="destructive"
      >
        <p class="text-sm">{{ t('channels.test.invalid') }}</p>
      </Alert>
      <Alert
        v-else-if="test.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(test.error.value) }}</p>
      </Alert>
      <Alert
        v-else-if="delivery.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(delivery.error.value) }}</p>
      </Alert>
      <p
        v-else-if="sending"
        class="text-sm text-muted"
      >
        {{ t('channels.test.sending') }}
      </p>
      <Alert
        v-else-if="result?.state === 'delivered'"
        color="success"
        data-test-result="delivered"
      >
        <p class="text-sm">{{ t('channels.test.delivered') }}</p>
        <p
          v-if="result.detail"
          class="mt-1 text-sm break-words"
        >
          {{ result.detail }}
        </p>
      </Alert>
      <Alert
        v-else-if="result"
        color="destructive"
        data-test-result="failed"
      >
        <p class="text-sm">{{ t('channels.test.failed') }}</p>
        <p
          v-if="result.error"
          class="mt-1 font-mono text-sm break-words"
        >
          {{ result.error }}
        </p>
      </Alert>
    </div>
  </div>
</template>
