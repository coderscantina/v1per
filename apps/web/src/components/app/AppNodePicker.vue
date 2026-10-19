<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/app/AppContextSwitcher.vue. One
// picker over the node tree replaces the workspace and site menus.
import Icon from '~/components/Icon.vue'
import NodeTreeList from '~/components/nodes/NodeTreeList.vue'
import { buttonVariants } from '~/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '~/components/ui/popover'
import { nodeSection, useCurrentNode } from '~/composables/useNodes'
import type { TreeNode } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * The node the shell is scoped to, as its path with slash separators: the last three levels
 * on a desk, the node alone on a phone. Ancestors the caller cannot read show as "…". Levels
 * that do not fit stay in the accessible name, so the button always reads as the full path.
 * Picking another node keeps the current section (overview, monitors, members, settings).
 */
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const { node, path, tree } = useCurrentNode()
const open = ref(false)

const SHOWN = 3
const segments = computed(() =>
  path.value.map((entry, at) => {
    const fromEnd = path.value.length - 1 - at
    return {
      entry,
      visibility: fromEnd >= SHOWN ? 'sr-only' : fromEnd > 0 ? '@max-lg:sr-only' : '',
    }
  })
)
const clipped = computed(() => path.value.length > SHOWN)
const initial = computed(() => node.value?.name?.trim().charAt(0).toUpperCase() || '?')

const select = (next: TreeNode) => {
  open.value = false
  void router.push({ name: nodeSection(route.name), params: { nodeId: next.id } })
}

const triggerClass = buttonVariants({ variant: 'fill', size: 'control', class: 'min-w-0 shrink font-medium' })
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger :class="triggerClass">
      <span
        class="grid size-5 shrink-0 place-items-center rounded-md bg-accent text-2xs font-bold text-accent-foreground"
        aria-hidden="true"
      >
        {{ initial }}
      </span>
      <span
        v-if="node"
        class="flex min-w-0 items-center gap-1.5"
      >
        <span class="sr-only">{{ t('nodes.picker.current') }} </span>
        <span
          v-if="clipped"
          class="text-muted @max-lg:hidden"
          aria-hidden="true"
        >… /</span>
        <template
          v-for="({ entry, visibility }, at) in segments"
          :key="entry.id"
        >
          <span
            class="truncate"
            :class="[visibility, at < segments.length - 1 ? 'max-w-36 text-muted' : 'text-primary']"
          >
            <template v-if="entry.name !== undefined">{{ entry.name }}</template>
            <template v-else>
              <span aria-hidden="true">…</span>
              <span class="sr-only">{{ t('nodes.hidden') }}</span>
            </template>
          </span>
          <span
            v-if="at < segments.length - 1"
            class="text-faint"
            :class="visibility"
          > / </span>
        </template>
      </span>
      <span
        v-else
        class="truncate text-muted"
      >
        {{ tree.loading.value ? t('nodes.picker.loading') : t('nodes.picker.choose') }}
      </span>
      <Icon
        name="lucide:chevrons-up-down"
        size="14"
        class="shrink-0 text-muted"
        aria-hidden="true"
      />
    </PopoverTrigger>
    <PopoverContent
      align="start"
      class="w-[min(22rem,calc(100vw-1.5rem))] p-0"
    >
      <NodeTreeList
        :index="tree.index.value"
        :label="t('nodes.picker.label')"
        :selected="node?.id"
        @select="select"
      />
    </PopoverContent>
  </Popover>
</template>
