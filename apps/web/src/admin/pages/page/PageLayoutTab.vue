<script setup lang="ts">
import { pagesResource } from '~/api/resources/pages'
import CommandAlert from '~/components/CommandAlert.vue'
import Icon from '~/components/Icon.vue'
import ComponentPickerDialog from '~/components/pages/ComponentPickerDialog.vue'
import SwitchField from '~/components/SwitchField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Checkbox } from '~/components/ui/checkbox'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { useIncidentTargets } from '~/composables/useIncidents'
import { useCurrentNode } from '~/composables/useNodes'
import { usePageEditor } from '~/composables/usePages'
import { PAGE_GROUPS_MAX } from '~/generated/limits'
import {
  addComponents,
  addGroup,
  canStep,
  moveComponent,
  moveGroup,
  placedKeys,
  removeComponent,
  removeGroup,
  stepComponent,
  updateComponent,
  updateGroup,
  useLayoutEditor,
  type Slot,
} from '~/lib/page-layout'
import { useI18n } from '~/plugins/i18n'

/**
 * The page's groups and their components. Components come from monitors and static components
 * on the page's node and below. Rows move by dragging the handle or with the arrow buttons,
 * which cross into the next group at an edge. The whole layout saves at once against the
 * page's revision; when someone saved first, the draft stays until reloaded.
 */
const { t } = useI18n()
const editor = usePageEditor()
const { tree } = useCurrentNode()
const detail = computed(() => editor.data.value ?? null)
const page = computed(() => detail.value?.page ?? null)
const layout = useLayoutEditor(detail, (args) => pagesResource.updateLayout(args))
const draft = layout.draft

const targets = useIncidentTargets(() => page.value?.node ?? null)
const ownNames = computed(() => new Map(targets.options.value.map((option) => [option.key, option.name] as const)))
const placed = computed(() => placedKeys(draft.value))

const announcement = ref('')
const focusMove = (key: string, direction: 'up' | 'down') =>
  void nextTick(() => {
    const preferred = document.querySelector<HTMLButtonElement>(`[data-move="${key}:${direction}"]`)
    const other = document.querySelector<HTMLButtonElement>(`[data-move="${key}:${direction === 'up' ? 'down' : 'up'}"]`)
    ;(preferred && !preferred.disabled ? preferred : other)?.focus()
  })

const step = (slot: Slot, delta: -1 | 1) => {
  const moved = draft.value[slot.group]?.components[slot.index]
  if (!moved) return
  draft.value = stepComponent(draft.value, slot, delta)
  const group = draft.value.findIndex((entry) => entry.components.some((component) => component.key === moved.key))
  const index = draft.value[group]?.components.findIndex((component) => component.key === moved.key) ?? 0
  announcement.value = t('pages.layout.movedTo', {
    name: moved.name,
    position: index + 1,
    group: draft.value[group]?.name || t('pages.layout.untitled'),
  })
  focusMove(moved.key, delta < 0 ? 'up' : 'down')
}

const stepGroup = (index: number, delta: -1 | 1) => {
  const group = draft.value[index]
  if (!group) return
  draft.value = moveGroup(draft.value, index, index + delta)
  announcement.value = t('pages.layout.groupMovedTo', { group: group.name || t('pages.layout.untitled'), position: index + delta + 1 })
  focusMove(group.key, delta < 0 ? 'up' : 'down')
}

const dragging = ref<Slot | null>(null)
const dropAt = ref<Slot | null>(null)
const startDrag = (event: DragEvent, slot: Slot) => {
  dragging.value = slot
  if (!event.dataTransfer) return
  event.dataTransfer.effectAllowed = 'move'
  event.dataTransfer.setData('text/plain', draft.value[slot.group]?.components[slot.index]?.name ?? '')
  const row = event.target instanceof HTMLElement ? event.target.closest('li') : null
  if (row) event.dataTransfer.setDragImage(row, 16, 16)
}
/** Over a row: before it in its upper half, after it in its lower half. */
const overRow = (event: DragEvent, slot: Slot) => {
  if (!dragging.value || !(event.currentTarget instanceof HTMLElement)) return
  const box = event.currentTarget.getBoundingClientRect()
  dropAt.value = { group: slot.group, index: event.clientY > box.top + box.height / 2 ? slot.index + 1 : slot.index }
}
const overGroup = (group: number) => {
  if (!dragging.value) return
  if (dropAt.value?.group !== group) dropAt.value = { group, index: draft.value[group]?.components.length ?? 0 }
}
const drop = () => {
  const from = dragging.value
  const to = dropAt.value
  if (from && to) {
    // `to.index` counts the row being moved while it is still in place.
    const index = from.group === to.group && from.index < to.index ? to.index - 1 : to.index
    draft.value = moveComponent(draft.value, from, { group: to.group, index })
  }
  endDrag()
}
const endDrag = () => {
  dragging.value = null
  dropAt.value = null
}
const dropBefore = (slot: Slot) => dropAt.value?.group === slot.group && dropAt.value.index === slot.index

const picking = ref<number | null>(null)
const pickerOpen = computed({
  get: () => picking.value !== null,
  set: (value: boolean) => {
    if (!value) picking.value = null
  },
})
const add = (picks: Parameters<typeof addComponents>[2]) => {
  if (picking.value !== null) draft.value = addComponents(draft.value, picking.value, picks)
}
const newGroup = () => {
  draft.value = addGroup(draft.value)
  const key = draft.value.at(-1)?.key
  void nextTick(() => document.querySelector<HTMLInputElement>(`[data-group-name="${key}"]`)?.focus())
}
const nodeName = (node: string) => tree.index.value.byId.get(node)?.name
const totalComponents = computed(() => placed.value.size)
</script>

<template>
  <Alert
    v-if="!editor.manage.value"
    icon="lucide:info"
  >
    <p class="text-sm">{{ t('pages.readOnly') }}</p>
  </Alert>
  <div
    v-else-if="page"
    class="grid gap-5"
  >
    <p class="text-sm text-muted">{{ t('pages.layout.intro') }}</p>

    <Alert
      v-if="layout.outdated.value && layout.dirty.value && !layout.conflict.value"
      color="warning"
    >
      <p class="text-sm">{{ t('pages.layout.outdated') }}</p>
      <Button
        type="button"
        variant="outline"
        size="sm"
        class="mt-2"
        @click="layout.reload()"
      >
        {{ t('pages.layout.discard') }}
      </Button>
    </Alert>

    <EmptyState
      v-if="draft.length === 0"
      icon="lucide:layout-list"
      :title="t('pages.layout.empty')"
      :description="t('pages.layout.emptyHint')"
    >
      <Button
        variant="accent"
        @click="picking = 0"
      >
        <Icon name="lucide:plus" />
        {{ t('pages.layout.addComponents') }}
      </Button>
    </EmptyState>

    <ol
      v-else
      class="grid gap-4"
      :aria-label="t('pages.layout.groups')"
    >
      <li
        v-for="(group, groupIndex) in draft"
        :key="group.key"
        class="rounded-panel border border-hairline bg-card"
        :data-group="group.name"
        @dragover.prevent="overGroup(groupIndex)"
        @drop.prevent="drop"
      >
        <div class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-hairline px-3 py-2.5 sm:px-4">
          <Input
            :model-value="group.name"
            :data-group-name="group.key"
            :aria-label="t('pages.layout.groupName')"
            :placeholder="t('pages.layout.groupNamePlaceholder')"
            class="h-8 min-w-0 flex-1 basis-40 font-medium"
            @update:model-value="(value) => (draft = updateGroup(draft, groupIndex, { name: String(value) }))"
          />
          <SwitchField
            :id="`group-open-${group.key}`"
            :model-value="group.defaultOpen"
            :label="t('pages.layout.defaultOpen')"
            @update:model-value="(value) => (draft = updateGroup(draft, groupIndex, { defaultOpen: value }))"
          />
          <div class="ml-auto flex shrink-0 gap-1">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              :data-move="`${group.key}:up`"
              :disabled="groupIndex === 0"
              :aria-label="t('pages.layout.groupUp', { group: group.name || t('pages.layout.untitled') })"
              @click="stepGroup(groupIndex, -1)"
            >
              <Icon name="lucide:arrow-up" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              :data-move="`${group.key}:down`"
              :disabled="groupIndex === draft.length - 1"
              :aria-label="t('pages.layout.groupDown', { group: group.name || t('pages.layout.untitled') })"
              @click="stepGroup(groupIndex, 1)"
            >
              <Icon name="lucide:arrow-down" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              :aria-label="t('pages.layout.removeGroup', { group: group.name || t('pages.layout.untitled') })"
              @click="draft = removeGroup(draft, groupIndex)"
            >
              <Icon name="lucide:trash-2" />
            </Button>
          </div>
        </div>

        <ol
          class="min-h-12 divide-y divide-divider"
          :aria-label="group.name || t('pages.layout.untitled')"
        >
          <li
            v-for="(component, index) in group.components"
            :key="component.key"
            class="relative flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2 sm:px-4"
            :class="dragging?.group === groupIndex && dragging.index === index ? 'opacity-50' : undefined"
            :data-component="component.name"
            @dragover.prevent.stop="overRow($event, { group: groupIndex, index })"
            @drop.prevent.stop="drop"
          >
            <span
              v-if="dropBefore({ group: groupIndex, index })"
              class="absolute inset-x-3 -top-px h-0.5 rounded-full bg-accent"
              aria-hidden="true"
            />
            <span
              draggable="true"
              class="flex size-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted hover:bg-fill active:cursor-grabbing max-sm:hidden"
              :title="t('pages.layout.dragHint')"
              aria-hidden="true"
              @dragstart="startDrag($event, { group: groupIndex, index })"
              @dragend="endDrag"
            >
              <Icon name="lucide:grip-vertical" />
            </span>
            <span class="flex min-w-0 flex-1 basis-32 items-center gap-2 text-sm text-primary">
              <Icon
                :name="component.target.type === 'monitor' ? 'lucide:activity' : 'lucide:box'"
                size="14"
                class="shrink-0 text-muted"
                aria-hidden="true"
              />
              <span class="truncate">{{ ownNames.get(component.key) ?? component.name }}</span>
            </span>
            <Input
              :model-value="component.displayName"
              :aria-label="t('pages.layout.displayNameFor', { name: ownNames.get(component.key) ?? component.name })"
              :placeholder="t('pages.layout.displayName')"
              class="h-8 min-w-0 flex-1 basis-40"
              @update:model-value="(value) => (draft = updateComponent(draft, { group: groupIndex, index }, { displayName: String(value) }))"
            />
            <label class="flex shrink-0 cursor-pointer items-center gap-2 text-sm text-foreground">
              <Checkbox
                :model-value="component.internal"
                @update:model-value="(value) => (draft = updateComponent(draft, { group: groupIndex, index }, { internal: value === true }))"
              />
              {{ t('pages.layout.internal') }}
            </label>
            <div class="ml-auto flex shrink-0 gap-1">
              <Button
                type="button"
                variant="ghost"
                size="icon"
                :data-move="`${component.key}:up`"
                :disabled="!canStep(draft, { group: groupIndex, index }, -1)"
                :aria-label="t('pages.layout.moveUp', { name: component.name })"
                @click="step({ group: groupIndex, index }, -1)"
              >
                <Icon name="lucide:arrow-up" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                :data-move="`${component.key}:down`"
                :disabled="!canStep(draft, { group: groupIndex, index }, 1)"
                :aria-label="t('pages.layout.moveDown', { name: component.name })"
                @click="step({ group: groupIndex, index }, 1)"
              >
                <Icon name="lucide:arrow-down" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                :aria-label="t('pages.layout.remove', { name: component.name })"
                @click="draft = removeComponent(draft, { group: groupIndex, index })"
              >
                <Icon name="lucide:x" />
              </Button>
            </div>
          </li>
          <li
            v-if="group.components.length === 0 || dropBefore({ group: groupIndex, index: group.components.length })"
            class="relative px-4 py-3 text-sm text-muted"
          >
            <span
              v-if="dropBefore({ group: groupIndex, index: group.components.length })"
              class="absolute inset-x-3 -top-px h-0.5 rounded-full bg-accent"
              aria-hidden="true"
            />
            <template v-if="group.components.length === 0">{{ t('pages.layout.groupEmpty') }}</template>
          </li>
        </ol>
        <div class="border-t border-hairline px-3 py-2 sm:px-4">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            @click="picking = groupIndex"
          >
            <Icon name="lucide:plus" />
            {{ t('pages.layout.addComponents') }}
          </Button>
        </div>
      </li>
    </ol>

    <div>
      <Button
        type="button"
        variant="fill"
        :disabled="draft.length >= PAGE_GROUPS_MAX"
        @click="newGroup"
      >
        <Icon name="lucide:folder-plus" />
        {{ t('pages.layout.addGroup') }}
      </Button>
    </div>

    <p
      class="sr-only"
      aria-live="polite"
    >
      {{ announcement }}
    </p>

    <div class="grid gap-4 border-t border-divider pt-5">
      <CommandAlert
        :error="layout.error.value"
        @reload="layout.reload()"
      />
      <div class="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="accent"
          :loading="layout.saving.value"
          :disabled="!layout.dirty.value"
          @click="layout.save()"
        >
          {{ t('pages.layout.save') }}
        </Button>
        <Button
          v-if="layout.dirty.value"
          type="button"
          variant="ghost"
          @click="layout.reload()"
        >
          {{ t('pages.layout.discard') }}
        </Button>
        <span class="text-sm text-muted">
          {{ layout.dirty.value ? t('pages.layout.unsaved') : t('pages.layout.count', { count: totalComponents }, totalComponents) }}
        </span>
      </div>
    </div>

    <ComponentPickerDialog
      v-model:open="pickerOpen"
      :group="draft[picking ?? 0]?.name ?? ''"
      :options="targets.options.value"
      :placed="placed"
      :node-name="nodeName"
      :page-node="page.node"
      @add="add"
    />
  </div>
</template>
