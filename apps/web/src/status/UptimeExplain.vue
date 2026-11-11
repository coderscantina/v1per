<script setup lang="ts">
import type { Explanation } from './uptime'

/**
 * An uptime figure that explains itself: the slot is the figure, and its explanation opens on
 * hover, on keyboard focus, and on click or tap, which keeps it open until the next one, Escape
 * or a click elsewhere. The panel hangs below the figure and moves sideways to stay in the window. `label` names the button for screen readers; the panel is its
 * description.
 */
const props = defineProps<{ explanation: Explanation; label: string; align?: 'start' | 'end' }>()

const id = useId()
const root = useTemplateRef<HTMLElement>('root')
const hovered = ref(false)
const focused = ref(false)
const pinned = ref(false)
const open = computed(() => hovered.value || focused.value || pinned.value)

const panel = useTemplateRef<HTMLElement>('panel')
/** Pixels the panel moves sideways to stay inside the window. */
const shift = ref(0)
const EDGE = 12
watch(open, async (shown) => {
  shift.value = 0
  if (!shown) return
  await nextTick()
  const box = panel.value?.getBoundingClientRect()
  if (!box) return
  const width = document.documentElement.clientWidth
  if (box.left < EDGE) shift.value = EDGE - box.left
  else if (box.right > width - EDGE) shift.value = width - EDGE - box.right
})

function close() {
  hovered.value = focused.value = pinned.value = false
}

function onFocus(event: FocusEvent) {
  // Keyboard focus only; a click focuses too and is handled as a click.
  if (event.target instanceof HTMLElement && event.target.matches(':focus-visible')) focused.value = true
}

function onOutside(event: PointerEvent) {
  if (event.target instanceof Node && !root.value?.contains(event.target)) pinned.value = false
}
onMounted(() => document.addEventListener('pointerdown', onOutside))
onBeforeUnmount(() => document.removeEventListener('pointerdown', onOutside))
</script>

<template>
  <span
    ref="root"
    class="relative inline-flex"
    @pointerenter="$event.pointerType === 'mouse' && (hovered = true)"
    @pointerleave="hovered = false"
    @keydown.esc="close"
  >
    <button
      type="button"
      data-testid="uptime-explain"
      :aria-label="props.label"
      :aria-expanded="open"
      :aria-describedby="open ? id : undefined"
      class="cursor-help rounded-sm text-left decoration-border-strong decoration-dotted underline-offset-4 hover:underline"
      @click="pinned = !pinned"
      @focus="onFocus"
      @blur="focused = false"
    >
      <slot />
    </button>
    <span
      v-if="open"
      :id="id"
      ref="panel"
      role="tooltip"
      :style="{ transform: `translateX(${shift}px)` }"
      data-testid="uptime-explanation"
      class="absolute top-full z-20 mt-2 flex w-72 max-w-[calc(100vw-3rem)] flex-col gap-2 rounded-lg border border-popover-border bg-popover px-3 py-2.5 text-left text-sm font-normal whitespace-normal text-popover-foreground shadow-soft-lg"
      :class="props.align === 'start' ? 'left-0' : 'right-0'"
    >
      <span class="flex flex-col gap-0.5">
        <span
          v-for="fact in explanation.facts"
          :key="fact"
          class="text-primary"
        >{{ fact }}</span>
      </span>
      <span
        v-if="explanation.rules.length > 0"
        class="flex flex-col gap-1 text-xs text-muted"
      >
        <span
          v-for="rule in explanation.rules"
          :key="rule"
        >{{ rule }}</span>
      </span>
    </span>
  </span>
</template>
