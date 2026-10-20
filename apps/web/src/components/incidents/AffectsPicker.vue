<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '~/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '~/components/ui/popover'
import type { TargetOption } from '~/composables/useIncidents'
import type { TargetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * The monitors and static components an incident names, as a row of chips that each come off
 * with one click, and an "Affects" button that opens a searchable list to pick more.
 */
const props = defineProps<{
  options: readonly TargetOption[]
  error?: string | null
  /** Names the node a target sits on, when it is below the incident's own. */
  nodeName?: (node: string) => string | undefined
}>()
const model = defineModel<TargetKey[]>({ required: true })

const { t } = useI18n()
const id = useId()
const open = ref(false)

const byKey = computed(() => new Map(props.options.map((option) => [option.key, option])))
const picked = computed(() => model.value.flatMap((key) => byKey.value.get(key) ?? []))
const remove = (key: TargetKey) => (model.value = model.value.filter((entry) => entry !== key))
const pick = (value: unknown) => {
  if (!Array.isArray(value)) return
  model.value = value.filter((key): key is TargetKey => typeof key === 'string' && byKey.value.has(key as TargetKey))
}
const kind = (option: TargetOption) => (option.kind === 'monitor' ? t('incidents.targets.monitor') : t('incidents.targets.component'))
</script>

<template>
  <div
    role="group"
    class="grid gap-1.5"
    :aria-label="t('incidents.composer.affects')"
    :aria-describedby="error ? `${id}-error` : undefined"
  >
    <div class="flex flex-wrap items-center gap-1.5">
      <span
        v-for="option in picked"
        :key="option.key"
        class="inline-flex h-7 max-w-full items-center gap-1.5 rounded-md bg-fill pr-1 pl-2 text-sm text-primary"
        :data-picked="option.name"
      >
        <Icon
          :name="option.kind === 'monitor' ? 'lucide:activity' : 'lucide:box'"
          size="13"
          class="shrink-0 text-muted"
          aria-hidden="true"
        />
        <span class="truncate">{{ option.name }}</span>
        <button
          type="button"
          class="grid size-5 shrink-0 cursor-pointer place-items-center rounded-sm text-muted hover:bg-fill-strong hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
          :aria-label="t('incidents.composer.remove', { name: option.name })"
          @click="remove(option.key)"
        >
          <Icon
            name="lucide:x"
            size="12"
          />
        </button>
      </span>

      <Popover v-model:open="open">
        <PopoverTrigger as-child>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            class="text-muted"
            :aria-invalid="error ? true : undefined"
          >
            <Icon name="lucide:plus" />
            {{ t('incidents.composer.affects') }}
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          class="w-80 p-0"
        >
          <Command
            multiple
            :model-value="model"
            @update:model-value="pick"
          >
            <CommandInput :placeholder="t('incidents.targets.search')" />
            <CommandList class="p-1">
              <p
                v-if="options.length === 0"
                class="px-2 py-6 text-center text-sm text-muted"
              >
                {{ t('incidents.targets.none') }}
              </p>
              <CommandEmpty class="text-muted">{{ t('incidents.composer.noMatch') }}</CommandEmpty>
              <CommandGroup>
                <CommandItem
                  v-for="option in options"
                  :key="option.key"
                  :value="option.key"
                  :data-target="option.name"
                  class="group"
                >
                  <Icon
                    :name="option.kind === 'monitor' ? 'lucide:activity' : 'lucide:box'"
                    class="text-muted group-data-[highlighted]:text-accent-foreground/75"
                    aria-hidden="true"
                  />
                  <span class="grid min-w-0 flex-1">
                    <span class="truncate">{{ option.name }}</span>
                    <span class="truncate text-xs text-muted group-data-[highlighted]:text-accent-foreground/75">
                      {{ kind(option) }}<template v-if="nodeName?.(option.node)"> · {{ nodeName(option.node) }}</template>
                    </span>
                  </span>
                  <Icon
                    v-if="model.includes(option.key)"
                    name="lucide:check"
                    aria-hidden="true"
                  />
                </CommandItem>
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
    </div>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-xs font-medium text-destructive"
    >
      {{ error }}
    </p>
  </div>
</template>
