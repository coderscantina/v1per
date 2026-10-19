<script setup lang="ts">
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import type { ProbeChoice, ProbeId } from '~/generated'
import { chosenProbes, pickProbes } from '~/lib/monitor-form'
import { probeStatusLabels } from '~/lib/probes'
import { useI18n } from '~/plugins/i18n'

/**
 * The probes that run a monitor's check, from those its node may use. The stored list stays as
 * it is until the person picks; an empty one shows the built-in probe as chosen, which it means.
 */
const props = defineProps<{ choices: readonly ProbeChoice[]; error?: string | null }>()
const model = defineModel<ProbeId[]>({ required: true })

const { t } = useI18n()

const picked = computed({
  get: () => chosenProbes(model.value, props.choices),
  set: (next: ProbeId[]) => (model.value = pickProbes(next, props.choices)),
})
const options = computed(() =>
  props.choices.map((choice) => ({
    value: choice.id,
    label: choice.name,
    hint: [t(probeStatusLabels[choice.status]), choice.own ? null : t('monitors.editor.probeInherited')]
      .filter((part) => part !== null)
      .join(' · '),
  }))
)
</script>

<template>
  <div class="grid gap-1.5">
    <CheckboxGroup
      v-model="picked"
      :legend="t('monitors.editor.probes')"
      :options="options"
      :error="error"
    />
    <p class="text-xs text-muted">{{ t('monitors.editor.probesHint') }}</p>
  </div>
</template>
