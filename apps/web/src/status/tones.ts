import type { HeartbeatStatus, IncidentState } from '~/generated'

import type { MessageKey } from './messages'
import type { IconName } from './icons'
import type { Tone } from './view'

/** The icon and word that go with each color, so color is never the only signal. */
export const toneIcon: Record<Tone, IconName> = {
  operational: 'check',
  maintenance: 'wrench',
  degraded: 'alert',
  partial_outage: 'alert',
  major_outage: 'down',
  none: 'minus',
}

export const toneLabel: Record<Tone, MessageKey> = {
  operational: 'status.operational',
  maintenance: 'status.maintenance',
  degraded: 'status.degraded',
  partial_outage: 'status.partial_outage',
  major_outage: 'status.major_outage',
  none: 'status.none',
}

/** A probe's last check, in page colors. */
export const probeTone: Record<HeartbeatStatus, Tone> = {
  up: 'operational',
  down: 'major_outage',
  pending: 'degraded',
  maintenance: 'maintenance',
}

export const probeLabel: Record<HeartbeatStatus, MessageKey> = {
  up: 'probe.up',
  down: 'probe.down',
  pending: 'probe.pending',
  maintenance: 'probe.maintenance',
}

export const stateLabel: Record<IncidentState, MessageKey> = {
  investigating: 'state.investigating',
  identified: 'state.identified',
  monitoring: 'state.monitoring',
  resolved: 'state.resolved',
}

/** The marker color of each incident state on the timeline. */
export const stateTone: Record<IncidentState, Tone> = {
  investigating: 'major_outage',
  identified: 'partial_outage',
  monitoring: 'maintenance',
  resolved: 'operational',
}
