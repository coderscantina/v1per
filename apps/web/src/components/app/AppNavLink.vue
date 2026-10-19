<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppNavLink.vue.
import Icon from '~/components/Icon.vue'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '~/components/ui/tooltip'
import type { NavigationItem } from '~/composables/useNavigation'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{
  item: NavigationItem
  /** Rail mode: icon only, label moved into a tooltip. */
  compact?: boolean
}>()

const emit = defineEmits<{ navigate: [] }>()

const { t } = useI18n()
const route = useRoute()
const label = computed(() => t(props.item.labelKey))
const below = computed(() => props.item.within?.includes(String(route.name)) ?? false)
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <Tooltip>
      <TooltipTrigger as-child>
        <RouterLink
          :to="item.to"
          class="group/nav relative flex h-shell-nav w-full items-center gap-3 rounded-control px-3 text-[0.9375rem] font-medium text-sidebar-muted transition-colors duration-150 ease-butter hover:bg-fill"
          :class="[compact && 'justify-center px-0', below && 'bg-nav-active! text-nav-active-foreground!']"
          :aria-current="below ? 'page' : undefined"
          exact-active-class="bg-nav-active! text-nav-active-foreground!"
          @click="emit('navigate')"
        >
          <Icon
            :name="item.icon"
            size="16"
            class="shrink-0 opacity-75 group-hover/nav:opacity-100"
            aria-hidden="true"
          />
          <span
            class="truncate"
            :class="compact && 'sr-only'"
          >
            {{ label }}
          </span>
        </RouterLink>
      </TooltipTrigger>
      <TooltipContent
        v-if="compact"
        side="right"
      >
        {{ label }}
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
</template>
