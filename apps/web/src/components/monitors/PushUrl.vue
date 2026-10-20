<script setup lang="ts">
import CopyButton from '~/components/CopyButton.vue'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { pushUrl } from '~/lib/monitor-form'
import { usePublicOrigin } from '~/composables/useMonitors'
import { useI18n } from '~/plugins/i18n'

/** A push monitor's URL, read only, with a copy button and how the sender uses it. */
const props = defineProps<{ id: string; token: string; label?: string }>()

const { t } = useI18n()
const origin = usePublicOrigin()
const url = computed(() => pushUrl(origin.value, props.token))
</script>

<template>
  <div class="grid gap-1.5">
    <Label :for="id">{{ label ?? t('monitors.detail.push') }}</Label>
    <div class="flex min-w-0 gap-2 max-sm:flex-col">
      <Input
        :id="id"
        :model-value="url"
        readonly
        class="min-w-0 font-mono text-sm"
        :aria-describedby="`${id}-description`"
        @focus="($event.target as HTMLInputElement).select()"
      />
      <CopyButton :text="url" />
    </div>
    <p
      :id="`${id}-description`"
      class="text-sm text-muted"
    >
      {{ t('monitors.detail.pushHint') }}
    </p>
  </div>
</template>
