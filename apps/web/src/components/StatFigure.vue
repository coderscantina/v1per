<script setup lang="ts">
/**
 * One figure in a `dl`: a quiet label and a value set larger and heavier, so the eye lands on the
 * number. `size` follows how much the figure matters on its surface; `labelBelow` puts the label
 * under the value, for a row of figures that should read as numbers first. The `label` slot
 * replaces the label text, for a marker beside it.
 */
withDefaults(
  defineProps<{ label: string; value?: string | null; size?: 'xl' | 'lg' | 'md'; title?: string; labelBelow?: boolean }>(),
  { value: null, size: 'lg', title: undefined, labelBelow: false }
)

const sizes = {
  xl: 'text-[2rem]/[2.375rem] tracking-tight',
  lg: 'text-[1.375rem]/[1.75rem] tracking-tight',
  md: 'text-base',
}
</script>

<template>
  <div class="grid min-w-0 content-start gap-0.5">
    <dt
      class="flex min-w-0 items-center gap-1.5 text-[0.8125rem] text-muted"
      :class="labelBelow && 'order-last'"
    >
      <slot name="label">
        <span class="truncate">{{ label }}</span>
      </slot>
    </dt>
    <dd
      class="truncate font-semibold text-primary tabular-nums"
      :class="sizes[size]"
      :title="title"
    >
      <slot>{{ value ?? '…' }}</slot>
    </dd>
  </div>
</template>
