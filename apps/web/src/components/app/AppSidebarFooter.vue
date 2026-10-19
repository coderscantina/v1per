<script setup lang="ts">
import type { RouteLocationRaw } from 'vue-router'

import Icon from '~/components/Icon.vue'
import { useSession } from '~/composables/useSession'
import type { IconName } from '~/lib/icons'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

/**
 * The sidebar's last row: the instance pages for instance admins and the running version. In
 * the collapsed rail the links keep only their icons.
 */
defineProps<{ compact?: boolean }>()
const emit = defineEmits<{ navigate: [] }>()

const { t } = useI18n()
const { account } = useSession()

const links = computed(() =>
  account.value?.instance_admin
    ? ([
        { id: 'instance', label: 'nav.sections.instance', icon: 'lucide:settings', to: { name: 'instance-settings' } },
        { id: 'imports', label: 'nav.imports', icon: 'lucide:import', to: { name: 'instance-imports' } },
      ] satisfies Array<{ id: string; label: MessageKey; icon: IconName; to: RouteLocationRaw }>)
    : []
)
</script>

<template>
  <div
    class="flex shrink-0 items-center gap-1 border-t border-sidebar-border pt-2.5 pb-3"
    :class="compact && 'flex-col'"
  >
    <nav
      v-if="links.length > 0"
      :aria-label="t('nav.sections.instance')"
      class="flex items-center gap-1"
      :class="compact && 'flex-col'"
    >
      <RouterLink
        v-for="link in links"
        :key="link.id"
        :to="link.to"
        class="flex h-8 items-center gap-[7px] rounded-lg px-2 text-[0.8125rem] text-sidebar-muted transition-colors hover:bg-fill hover:text-sidebar-accent-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        active-class="bg-nav-active text-nav-active-foreground"
        :aria-label="compact ? t(link.label) : undefined"
        @click="emit('navigate')"
      >
        <Icon
          :name="link.icon"
          size="14"
          aria-hidden="true"
        />
        <span v-if="!compact">{{ t(link.label) }}</span>
      </RouterLink>
    </nav>
    <span
      v-if="runtimeConfig.version && !compact"
      class="ml-auto pr-1 text-2xs text-sidebar-muted"
    >
      {{ t('shell.versionShort', { version: runtimeConfig.version }) }}
    </span>
  </div>
</template>
