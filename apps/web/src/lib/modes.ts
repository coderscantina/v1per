import type { RouteLocationRaw } from 'vue-router'

import type { MonitorId, NodeId } from '~/generated'

/**
 * The admin app's two ways of working on a node: observe what happens and react to it, or
 * configure what is checked, who is told and who has access. Every node page belongs to one.
 */
export type Mode = 'observe' | 'configure'
export const modes = ['observe', 'configure'] as const satisfies readonly Mode[]

/** A node's configuration, one tab each, in tab order. */
export const configureSections = [
  'node-monitors',
  'node-pages',
  'node-components',
  'node-channels',
  'node-probes',
  'node-maintenance',
  'node-members',
  'node-settings',
] as const
export type ConfigureSection = (typeof configureSections)[number]
export const isConfigureSection = (name: unknown): name is ConfigureSection => configureSections.some((entry) => entry === name)

/** Where a node leads in a mode: its dashboard, or `section` of its configuration. */
export const nodeTarget = (node: NodeId, mode: Mode, section: ConfigureSection): RouteLocationRaw => ({
  name: mode === 'observe' ? 'node' : section,
  params: { nodeId: node },
})

/** Where a monitor leads in a mode: its page, or its editor for those who may change it. */
export const monitorTarget = (node: NodeId, monitor: MonitorId, mode: Mode, manage: boolean): RouteLocationRaw => ({
  name: mode === 'configure' && manage ? 'node-monitor-edit' : 'node-monitor',
  params: { nodeId: node, monitorId: monitor },
})

/** The page the mode switch starts from. */
export interface Place {
  name: string
  node: NodeId | null
  monitor: MonitorId | null
}

/**
 * Where the mode switch leads: a monitor's page and its editor swap, anything else goes to the
 * node's dashboard or to `section` of its configuration. `null` without a node to go to.
 */
export function modeTarget(place: Place, mode: Mode, section: ConfigureSection): RouteLocationRaw | null {
  if (!place.node) return null
  if (place.monitor && (place.name === 'node-monitor' || place.name === 'node-monitor-edit')) {
    return monitorTarget(place.node, place.monitor, mode, true)
  }
  return nodeTarget(place.node, mode, section)
}
