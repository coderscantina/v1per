<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppSidebarNav.vue.
import AppNavLink from '~/components/app/AppNavLink.vue'
import { useNavigation } from '~/composables/useNavigation'
import { useI18n } from '~/plugins/i18n'

defineProps<{ compact?: boolean }>()

const emit = defineEmits<{ navigate: [] }>()

const { t } = useI18n()
const { sections } = useNavigation()
</script>

<template>
  <nav
    class="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2"
    :class="compact ? 'gap-4 pt-2' : 'gap-6 pt-4'"
    :aria-label="t('nav.label')"
  >
    <div
      v-for="(section, index) in sections"
      :key="section.id"
      class="flex flex-col gap-shell-gap"
      :class="compact ? 'px-3' : 'px-4'"
    >
      <h2
        v-if="!compact"
        class="truncate px-2.5 pb-1.5 text-[0.8125rem] leading-5 text-muted"
      >
        {{ t(section.heading) }}
      </h2>
      <hr
        v-else-if="index > 0"
        class="mx-2 mb-1 border-hairline"
      >
      <AppNavLink
        v-for="item in section.items"
        :key="item.id"
        :item="item"
        :compact="compact"
        @navigate="emit('navigate')"
      />
    </div>
  </nav>
</template>
