<script lang="ts" setup>
import { cva, type VariantProps } from 'class-variance-authority'
import type { HTMLAttributes } from 'vue'

import Icon from '~/components/Icon.vue'
import type { IconName } from '~/lib/icons'
import { cn } from '~/lib/utils'

/**
 * A message on a faint wash of its colour with a glowing bar at the left. `color` sets `--tone`;
 * only the bar, the wash and the icon carry it, the text keeps the regular ink.
 */
const alertVariants = cva(
  'relative w-full rounded-r-lg bg-[color-mix(in_oklab,var(--tone)_11%,var(--card))] p-3 pl-4 text-foreground before:absolute before:inset-y-1.5 before:left-0 before:w-0.5 before:rounded-full before:bg-(--tone) before:shadow-[0_0_10px_var(--tone)]',
  {
    variants: {
      color: {
        default: '[--tone:var(--faint)]',
        info: '[--tone:var(--info)]',
        destructive: '[--tone:var(--destructive)]',
        success: '[--tone:var(--success)]',
        warning: '[--tone:var(--warning)]',
      },
    },
    defaultVariants: {
      color: 'default',
    },
  }
)

interface AlertProps {
  icon?: IconName
  color?: VariantProps<typeof alertVariants>['color']
  class?: HTMLAttributes['class']
}

const props = withDefaults(defineProps<AlertProps>(), {
  color: 'default',
})
</script>

<template>
  <div
    :class="cn(alertVariants({ color }), props.class)"
    role="alert"
  >
    <div class="flex items-start gap-2">
      <Icon
        v-if="icon"
        :name="icon"
        class="mt-0.5 size-4.5 shrink-0 text-(--tone)"
      />
      <div>
        <slot />
      </div>
    </div>
  </div>
</template>
