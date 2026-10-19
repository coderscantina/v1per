<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'
import { useMode } from '~/composables/useMode'
import type { IconName } from '~/lib/icons'
import type { Mode } from '~/lib/modes'
import { cn } from '~/lib/utils'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * Observe or configure, as two links to the current page's counterpart in each mode. `stacked`
 * draws them as the phone's tab bar items. The layout binds `o` and `c` to the same targets.
 */
defineProps<{ stacked?: boolean }>()

const { t } = useI18n()
const { mode, targets } = useMode()

const entries: Array<{ mode: Mode; label: MessageKey; icon: IconName }> = [
  { mode: 'observe', label: 'shell.modes.observe', icon: 'lucide:activity' },
  { mode: 'configure', label: 'shell.modes.configure', icon: 'lucide:sliders-horizontal' },
]

const links = computed(() =>
  entries.flatMap((entry) => {
    const to = targets.value[entry.mode]
    return to ? [{ ...entry, to, current: mode.value === entry.mode }] : []
  })
)
</script>

<template>
  <nav
    v-if="!stacked"
    :aria-label="t('shell.modes.label')"
    :class="segmentedTrack"
  >
    <RouterLink
      v-for="link in links"
      :key="link.mode"
      :to="link.to"
      :data-state="link.current ? 'on' : 'off'"
      :aria-current="link.current ? 'page' : undefined"
      :class="cn(segmentedItem({ size: 'md' }), 'gap-[7px]')"
    >
      <Icon
        :name="link.icon"
        size="15"
        aria-hidden="true"
        :class="link.current && (link.mode === 'observe' ? 'text-status-up' : 'text-accent-text')"
      />
      {{ t(link.label) }}
    </RouterLink>
  </nav>
  <template v-else>
    <li
      v-for="link in links"
      :key="link.mode"
    >
      <RouterLink
        :to="link.to"
        class="group/tab flex h-full flex-col items-center justify-center gap-1 text-2xs font-medium text-sidebar-muted transition-colors duration-200 ease-butter active:text-sidebar-accent-foreground"
        :class="link.current && 'is-active text-sidebar-accent-foreground'"
        :aria-current="link.current ? 'page' : undefined"
      >
        <span class="grid h-7 w-12 place-items-center rounded-full transition-colors duration-200 ease-butter group-[.is-active]/tab:bg-sidebar-accent">
          <Icon
            :name="link.icon"
            size="20"
            aria-hidden="true"
          />
        </span>
        <span class="max-w-full truncate px-1">{{ t(link.label) }}</span>
      </RouterLink>
    </li>
  </template>
</template>
