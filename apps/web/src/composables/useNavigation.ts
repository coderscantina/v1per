import { computed } from 'vue'
import type { RouteLocationRaw } from 'vue-router'

import type { IconName } from '~/lib/icons'
import type { MessageKey } from '~/plugins/i18n'

import { useCurrentNode } from './useNodes'
import { useSession } from './useSession'

export interface NavigationItem {
  id: string
  labelKey: MessageKey
  icon: IconName
  to: RouteLocationRaw
  /** Routes below this item, such as one monitor under Monitors, that mark it current too. */
  within?: readonly string[]
}

export interface NavigationSection {
  id: string
  heading: MessageKey
  items: NavigationItem[]
}

/**
 * Every page of the admin app, for the command palette. Node pages are scoped to the current
 * node; in the shell the tree, the mode switch and the configure tabs reach them.
 */
export function useNavigation() {
  const { node } = useCurrentNode()
  const { account } = useSession()

  const sections = computed<NavigationSection[]>(() => {
    const list: NavigationSection[] = []
    const current = node.value
    if (current) {
      const params = { nodeId: current.id }
      list.push({
        id: 'node',
        heading: 'nav.sections.node',
        items: [
          { id: 'node', labelKey: 'nav.overview', icon: 'lucide:layout-grid', to: { name: 'node', params } },
          {
            id: 'node-monitors',
            labelKey: 'nav.monitors',
            icon: 'lucide:activity',
            to: { name: 'node-monitors', params },
            within: ['node-monitor', 'node-monitor-new', 'node-monitor-edit'],
          },
          {
            id: 'node-incidents',
            labelKey: 'nav.incidents',
            icon: 'lucide:siren',
            to: { name: 'node-incidents', params },
            within: ['node-incident', 'node-incident-new', 'node-incident-edit'],
          },
          {
            id: 'node-maintenance',
            labelKey: 'nav.maintenance',
            icon: 'lucide:wrench',
            to: { name: 'node-maintenance', params },
            within: ['node-maintenance-new', 'node-maintenance-edit'],
          },
          {
            id: 'node-pages',
            labelKey: 'nav.pages',
            icon: 'lucide:globe',
            to: { name: 'node-pages', params },
            within: ['node-page', 'node-page-layout', 'node-page-access', 'node-page-hostnames', 'node-page-preview', 'node-page-incidents'],
          },
          { id: 'node-members', labelKey: 'nav.members', icon: 'lucide:users', to: { name: 'node-members', params } },
          { id: 'node-probes', labelKey: 'nav.probes', icon: 'lucide:radio-tower', to: { name: 'node-probes', params } },
          {
            id: 'node-channels',
            labelKey: 'nav.channels',
            icon: 'lucide:bell',
            to: { name: 'node-channels', params },
            within: ['node-channel', 'node-channel-new', 'node-channel-edit'],
          },
          { id: 'node-components', labelKey: 'nav.components', icon: 'lucide:box', to: { name: 'node-components', params } },
          { id: 'node-settings', labelKey: 'nav.settings', icon: 'lucide:settings-2', to: { name: 'node-settings', params } },
        ],
      })
    }
    if (account.value?.instance_admin) {
      list.push({
        id: 'instance',
        heading: 'nav.sections.instance',
        items: [
          { id: 'instance', labelKey: 'nav.instance', icon: 'lucide:server', to: { name: 'instance-settings' } },
          {
            id: 'instance-imports',
            labelKey: 'nav.imports',
            icon: 'lucide:import',
            to: { name: 'instance-imports' },
            within: ['instance-import'],
          },
        ],
      })
    }
    return list
  })

  const items = computed(() => sections.value.flatMap((section) => section.items))

  return { sections, items }
}
