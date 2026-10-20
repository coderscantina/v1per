<script lang="ts" setup>
import { cva, type VariantProps } from 'class-variance-authority'
import type { HTMLAttributes } from 'vue'

import Icon from '~/components/Icon.vue'
import type { IconName } from '~/lib/icons'
import { cn } from '~/lib/utils'

/**
 * A message on a faint tint of its colour, edged by a hairline of the same colour. `color` sets
 * `--tone`; the tint, the edge and the icon carry it. Text is the strongest ink, and muted text
 * inside turns into that ink at reduced opacity, so it sits on the tint instead of graying out.
 */
const alertVariants = cva(
  'w-full rounded-control border border-(--tone)/30 bg-(--tone)/8 px-3.5 py-3 text-primary [--muted:color-mix(in_oklab,var(--primary)_72%,transparent)]',
  {
    variants: {
      color: {
        default: '[--tone:var(--status-none)]',
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
    <div class="flex items-start gap-2.5">
      <Icon
        v-if="icon"
        :name="icon"
        class="mt-0.5 size-4 shrink-0 text-(--tone)"
      />
      <div class="min-w-0 select-text">
        <slot />
      </div>
    </div>
  </div>
</template>
