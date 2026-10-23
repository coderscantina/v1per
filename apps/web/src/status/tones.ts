import type { HeartbeatStatus, IncidentState, SlaPeriodKind } from '~/generated'

import type { MessageKey } from './messages'
import type { IconName } from './icons'
import type { SlaState } from './sla'
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

/** SLA states in page colors: kept and on track read healthy, no data reads absent, never missed. */
export const slaTone: Record<SlaState, Tone> = {
  met: 'operational',
  on_track: 'operational',
  at_risk: 'degraded',
  missed: 'major_outage',
  no_data: 'none',
}

export const slaIcon: Record<SlaState, IconName> = {
  met: 'check',
  on_track: 'check',
  at_risk: 'alert',
  missed: 'down',
  no_data: 'minus',
}

export const slaLabel: Record<SlaState, MessageKey> = {
  met: 'sla.state.met',
  on_track: 'sla.state.on_track',
  at_risk: 'sla.state.at_risk',
  missed: 'sla.state.missed',
  no_data: 'sla.state.no_data',
}

export const periodKindLabel: Record<SlaPeriodKind, MessageKey> = {
  month: 'sla.period.month',
  quarter: 'sla.period.quarter',
  year: 'sla.period.year',
}
