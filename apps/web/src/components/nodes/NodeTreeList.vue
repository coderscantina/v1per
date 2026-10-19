<script setup lang="ts">
import { ListboxContent, ListboxFilter, ListboxItem, ListboxRoot } from 'reka-ui'

import Icon from '~/components/Icon.vue'
import type { NodeId, TreeNode } from '~/generated'
import { pickerRows, type TreeIndex } from '~/lib/node-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * The tree as a searchable list box: arrows move, Enter picks, typing filters by name and by
 * ancestor name. Placeholders keep the tree's shape and cannot be picked. `refuse` returns why
 * a readable node cannot be picked here (a move target, a key's node); the reason shows under
 * the name. Built on Reka's list box, styled like Kit's command list.
 */
const props = defineProps<{
  index: TreeIndex
  label: string
  selected?: NodeId | null
  refuse?: (node: TreeNode) => string | null
}>()
const emit = defineEmits<{ select: [node: TreeNode] }>()

const { t } = useI18n()
const query = ref('')

const rows = computed(() =>
  pickerRows(props.index, query.value).map((row) => ({
    ...row,
    reason: row.readable ? (props.refuse?.(row.node) ?? null) : null,
  }))
)

const trail = (nodes: TreeNode[]) => nodes.map((node) => node.name ?? '…').join(' / ')

const pick = (value: unknown) => {
  const row = rows.value.find((entry) => entry.node.id === value)
  if (row?.readable && !row.reason) emit('select', row.node)
}
</script>

<template>
  <ListboxRoot
    :model-value="selected ?? undefined"
    class="flex min-h-0 flex-col"
    highlight-on-hover
    selection-behavior="replace"
    @update:model-value="pick"
  >
    <div class="flex items-center gap-2 border-b border-hairline px-3">
      <Icon
        name="lucide:search"
        class="shrink-0 text-muted"
        aria-hidden="true"
      />
      <ListboxFilter
        v-model="query"
        auto-focus
        :aria-label="t('nodes.picker.search')"
        :placeholder="t('nodes.picker.search')"
        class="flex h-10 w-full bg-transparent py-3 text-sm text-primary outline-none placeholder:text-muted"
      />
    </div>
    <ListboxContent
      :aria-label="label"
      class="max-h-[min(24rem,60vh)] overflow-x-hidden overflow-y-auto p-1"
    >
      <ListboxItem
        v-for="row in rows"
        :key="row.node.id"
        :value="row.node.id"
        :disabled="!row.readable || !!row.reason"
        :aria-disabled="!row.readable || !!row.reason ? 'true' : undefined"
        class="group/row relative flex cursor-pointer items-center gap-2 rounded-sm py-1.5 pr-2 text-sm outline-none select-none data-[disabled]:cursor-default data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
        :style="{ paddingLeft: `${0.5 + Math.min(row.indent, 6) * 0.875}rem` }"
      >
        <Icon
          :name="row.readable ? 'lucide:folder' : 'lucide:folder-lock'"
          size="15"
          class="shrink-0 text-muted group-data-[highlighted]/row:text-accent-foreground"
          aria-hidden="true"
        />
        <span class="grid min-w-0 flex-1">
          <span
            v-if="row.readable"
            class="truncate"
            :class="row.reason ? 'text-muted' : 'text-primary group-data-[highlighted]/row:text-accent-foreground'"
          >
            {{ row.node.name }}
          </span>
          <span
            v-else
            class="truncate text-muted"
          >
            {{ t('nodes.hidden') }}
          </span>
          <span
            v-if="row.trail.length > 0"
            class="truncate text-xs text-muted group-data-[highlighted]/row:text-accent-foreground"
          >
            {{ trail(row.trail) }}
          </span>
          <span
            v-if="row.reason"
            class="truncate text-xs text-muted"
          >
            {{ row.reason }}
          </span>
        </span>
        <Icon
          v-if="row.node.id === selected"
          name="lucide:check"
          size="15"
          class="shrink-0"
          aria-hidden="true"
        />
      </ListboxItem>
      <p
        v-if="rows.length === 0"
        class="px-3 py-6 text-center text-sm text-muted"
      >
        {{ query ? t('nodes.picker.noMatch') : t('nodes.picker.empty') }}
      </p>
    </ListboxContent>
  </ListboxRoot>
</template>
