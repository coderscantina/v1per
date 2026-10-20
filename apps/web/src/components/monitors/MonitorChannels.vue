<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import RailSection from '~/components/observe/RailSection.vue'
import { Skeleton } from '~/components/ui/skeleton'
import { useAvailableChannels } from '~/composables/useChannels'
import type { ChannelId, NodeId } from '~/generated'
import { providerIcon, providerName } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

/**
 * The channels that alert about a monitor, live, each with its provider and whether it sits on
 * the monitor's node or on one above.
 */
const props = defineProps<{ node: NodeId; nodeName?: string; channels: ChannelId[] }>()

const { t } = useI18n()
const available = useAvailableChannels(() => props.node)
const list = computed(() => {
  const choices = available.data.value?.channels
  if (!choices) return null
  const picked = new Set(props.channels)
  return choices.filter((choice) => picked.has(choice.id))
})
</script>

<template>
  <RailSection
    id="monitor-channels"
    :title="t('monitors.detail.told')"
    :meta="list ? String(list.length) : undefined"
  >
    <Skeleton
      v-if="!list && channels.length > 0"
      class="h-12 rounded-control"
    />
    <p
      v-else-if="!list || list.length === 0"
      class="text-[0.8125rem] text-muted"
    >
      {{ t('monitors.detail.toldNone') }}
    </p>
    <ul
      v-else
      class="divide-y divide-hairline overflow-hidden rounded-control border border-hairline bg-card"
    >
      <li
        v-for="channel in list"
        :key="channel.id"
        class="flex items-start gap-2.5 px-3 py-2.5 text-[0.8125rem]"
      >
        <Icon
          :name="providerIcon(channel.provider)"
          size="14"
          class="mt-0.5 shrink-0 text-muted"
          aria-hidden="true"
        />
        <div class="grid min-w-0 flex-1">
          <span class="break-words text-primary">{{ channel.name }}</span>
          <span class="text-muted">
            {{
              channel.own && nodeName
                ? t('monitors.detail.toldFrom', { provider: providerName(channel.provider, t), node: nodeName })
                : t('monitors.detail.toldAbove', { provider: providerName(channel.provider, t) })
            }}
          </span>
        </div>
      </li>
    </ul>
  </RailSection>
</template>
