<script setup lang="ts">
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import type { TargetOption } from '~/composables/useIncidents'
import type { Target } from '~/generated'
import { parseTargetKey, type TargetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * Picks monitors and static components from the page's node and below for one group. Those
 * already on the page are left out, since a page shows each once.
 */
const props = defineProps<{
  group: string
  options: TargetOption[]
  placed: Set<TargetKey>
  /** Node names for entries below the page's node. */
  nodeName: (node: string) => string | undefined
  pageNode: string
}>()
const emit = defineEmits<{ add: [picks: { target: Target; name: string }[]] }>()
const open = defineModel<boolean>('open', { default: false })

const { t, locale } = useI18n()
// One list per group: each group's "select all" sets only its own.
const pickedMonitors = ref<TargetKey[]>([])
const pickedComponents = ref<TargetKey[]>([])
const picked = computed(() => [...pickedMonitors.value, ...pickedComponents.value])
watch(open, (value) => {
  if (!value) return
  pickedMonitors.value = []
  pickedComponents.value = []
})

const choices = computed(() => {
  const byKind = (kind: TargetOption['kind']) =>
    props.options
      .filter((option) => option.kind === kind && !props.placed.has(option.key))
      .map((option) => ({
        value: option.key,
        label: option.name,
        hint: option.node === props.pageNode ? undefined : props.nodeName(option.node),
      }))
      .sort((a, b) => a.label.localeCompare(b.label, locale.value))
  return { monitors: byKind('monitor'), components: byKind('component') }
})
const names = computed(() => new Map(props.options.map((option) => [option.key, option.name] as const)))

const add = () => {
  emit(
    'add',
    picked.value.map((key) => ({ target: parseTargetKey(key), name: names.value.get(key) ?? '' }))
  )
  open.value = false
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-lg">
      <DialogHeaderCombined
        :title="t('pages.layout.addTitle', { group: group || t('pages.layout.untitled') })"
        :description="t('pages.layout.addDescription')"
      />
      <div class="grid max-h-[60vh] gap-5 overflow-y-auto pr-1">
        <p
          v-if="choices.monitors.length === 0 && choices.components.length === 0"
          class="text-sm text-muted"
        >
          {{ t('pages.layout.nothingToAdd') }}
        </p>
        <CheckboxGroup
          v-if="choices.monitors.length > 0"
          v-model="pickedMonitors"
          :legend="t('pages.layout.monitors')"
          :options="choices.monitors"
        />
        <CheckboxGroup
          v-if="choices.components.length > 0"
          v-model="pickedComponents"
          :legend="t('pages.layout.staticComponents')"
          :options="choices.components"
        />
      </div>
      <div class="flex justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          @click="open = false"
        >
          {{ t('actions.cancel') }}
        </Button>
        <Button
          type="button"
          variant="primary"
          :disabled="picked.length === 0"
          @click="add"
        >
          {{ t('pages.layout.addCount', { count: picked.length }, picked.length) }}
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
