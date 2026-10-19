<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { templateExample, templateVariables } from '~/lib/channel-form'
import { templateVariableHints } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

/**
 * Kuma's template variables next to a template field, folded until opened. Templates use
 * Liquid, so imported Kuma templates keep producing the same messages.
 */
defineProps<{ id: string }>()

const { t } = useI18n()
</script>

<template>
  <details
    :id="id"
    class="group rounded-lg border border-hairline bg-fill/40 text-sm"
  >
    <summary
      class="flex cursor-pointer list-none items-center gap-2 rounded-lg px-3 py-2 font-medium text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden"
    >
      <Icon
        name="lucide:chevron-right"
        size="14"
        class="transition-transform group-open:rotate-90"
        aria-hidden="true"
      />
      {{ t('channels.templates.title') }}
    </summary>
    <div class="grid gap-3 border-t border-hairline px-3 py-3">
      <p class="text-muted">{{ t('channels.templates.intro') }}</p>
      <dl class="grid gap-x-4 gap-y-1.5 sm:grid-cols-[auto_1fr]">
        <template
          v-for="variable in templateVariables"
          :key="variable"
        >
          <dt><code class="rounded bg-fill px-1.5 py-0.5 font-mono text-xs text-primary">{{ variable }}</code></dt>
          <dd class="text-foreground max-sm:mb-1">{{ t(templateVariableHints[variable]) }}</dd>
        </template>
      </dl>
      <div class="grid gap-1">
        <p class="text-muted">{{ t('channels.templates.example') }}</p>
        <code class="block rounded bg-fill px-2 py-1.5 font-mono text-xs break-all text-primary">{{ templateExample }}</code>
      </div>
      <p class="text-xs text-muted">{{ t('channels.templates.noMonitor') }}</p>
    </div>
  </details>
</template>
