<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/CommandPalette.vue. Offers the same
// navigation as the sidebar, the current node's monitors and the shell's actions.
import Icon from '~/components/Icon.vue'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '~/components/ui/command'
import { DialogDescription, DialogTitle } from '~/components/ui/dialog'
import { useColorMode } from '~/composables/useColorMode'
import { useCommandPalette } from '~/composables/useCommandPalette'
import { useNodeMonitors } from '~/composables/useMonitors'
import { useNavigation } from '~/composables/useNavigation'
import { useCurrentNode } from '~/composables/useNodes'
import { useSignOut } from '~/composables/useSession'
import { useShortcut, useShortcutHelp } from '~/composables/useShortcuts'
import { useSidebar } from '~/composables/useSidebar'
import { can } from '~/lib/access'
import type { IconName } from '~/lib/icons'
import { formatShortcut } from '~/lib/shortcuts'
import { presenceLabels, presenceOf } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const router = useRouter()
const palette = useCommandPalette()
const sidebar = useSidebar()
const { isDark, setMode } = useColorMode()
const shortcutHelp = useShortcutHelp()
const signOut = useSignOut()
const { items } = useNavigation()
const { node } = useCurrentNode()
/** Read only while the palette is open, so a closed palette holds no stream. */
const monitors = useNodeMonitors(() => (palette.isOpen.value ? (node.value?.id ?? null) : null))

interface Action {
  id: string
  label: string
  icon: IconName
  /** Keys as `useShortcut` takes them, e.g. `mod+b`. */
  shortcut?: string
  run: () => void
}

const actions = computed<Action[]>(() => [
  ...(node.value && can(node.value, 'monitors.manage')
    ? [
        {
          id: 'add-monitor',
          label: t('commandPalette.addMonitor'),
          icon: 'lucide:plus',
          run: () => void router.push({ name: 'node-monitor-new', params: { nodeId: node.value?.id } }),
        } satisfies Action,
      ]
    : []),
  ...(node.value && can(node.value, 'incidents.manage')
    ? [
        {
          id: 'open-incident',
          label: t('commandPalette.openIncident'),
          icon: 'lucide:siren',
          run: () => void router.push({ name: 'node-incident-new', params: { nodeId: node.value?.id } }),
        } satisfies Action,
      ]
    : []),
  ...(node.value && can(node.value, 'maintenance.manage')
    ? [
        {
          id: 'plan-maintenance',
          label: t('commandPalette.planMaintenance'),
          icon: 'lucide:wrench',
          run: () => void router.push({ name: 'node-maintenance-new', params: { nodeId: node.value?.id } }),
        } satisfies Action,
      ]
    : []),
  {
    id: 'theme',
    label: isDark.value ? t('commandPalette.lightMode') : t('commandPalette.darkMode'),
    icon: isDark.value ? 'lucide:sun' : 'lucide:moon',
    run: () => setMode(isDark.value ? 'light' : 'dark'),
  },
  {
    id: 'sidebar',
    label: t('shortcuts.toggleSidebar'),
    icon: 'lucide:panel-left',
    shortcut: 'mod+b',
    run: () => sidebar.toggle(),
  },
  { id: 'shortcuts', label: t('shortcuts.title'), icon: 'lucide:keyboard', run: () => shortcutHelp.show() },
  { id: 'sign-out', label: t('auth.signOut'), icon: 'lucide:log-out', run: () => void signOut() },
])

useShortcut({
  keys: 'mod+k',
  description: () => t('commandPalette.open'),
  allowInInput: true,
  allowInOverlay: true,
  handler: () => palette.toggle(),
})

const run = (action: () => void) => {
  palette.close()
  action()
}
</script>

<template>
  <CommandDialog v-model:open="palette.isOpen.value">
    <DialogTitle class="sr-only">{{ t('commandPalette.title') }}</DialogTitle>
    <DialogDescription class="sr-only">{{ t('commandPalette.description') }}</DialogDescription>
    <CommandInput :placeholder="t('commandPalette.placeholder')" />
    <CommandList>
      <CommandEmpty>{{ t('commandPalette.empty') }}</CommandEmpty>
      <CommandGroup :heading="t('commandPalette.navigation')">
        <CommandItem
          v-for="item in items"
          :key="item.id"
          :value="`${t(item.labelKey)} ${item.id}`"
          @select="run(() => router.push(item.to))"
        >
          <Icon :name="item.icon" />
          {{ t(item.labelKey) }}
        </CommandItem>
      </CommandGroup>
      <CommandGroup
        v-if="node && (monitors.data.value?.monitors.length ?? 0) > 0"
        :heading="t('commandPalette.monitors', { node: node.name ?? '' })"
      >
        <CommandItem
          v-for="monitor in monitors.data.value?.monitors ?? []"
          :key="monitor.id"
          :value="`${monitor.name} ${monitor.tags.join(' ')} monitor-${monitor.id}`"
          @select="run(() => router.push({ name: 'node-monitor', params: { nodeId: node?.id, monitorId: monitor.id } }))"
        >
          <Icon name="lucide:activity" />
          <span class="min-w-0 flex-1 truncate">{{ monitor.name }}</span>
          <span class="text-xs text-muted">{{ t(presenceLabels[presenceOf(monitor)]) }}</span>
        </CommandItem>
      </CommandGroup>
      <CommandGroup :heading="t('commandPalette.actions')">
        <CommandItem
          v-for="action in actions"
          :key="action.id"
          :value="action.label"
          @select="run(action.run)"
        >
          <Icon :name="action.icon" />
          {{ action.label }}
          <CommandShortcut v-if="action.shortcut">{{ formatShortcut(action.shortcut) }}</CommandShortcut>
        </CommandItem>
      </CommandGroup>
    </CommandList>
  </CommandDialog>
</template>
