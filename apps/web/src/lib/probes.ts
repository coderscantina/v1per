import type { Probe, ProbeStatus } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

export const probeStatusLabels: Record<ProbeStatus, MessageKey> = {
  waiting: 'probes.status.waiting',
  online: 'probes.status.online',
  offline: 'probes.status.offline',
  revoked: 'probes.status.revoked',
}

/** The dot colour; the word always goes with it. */
export const probeStatusFill: Record<ProbeStatus, string> = {
  waiting: 'bg-status-pending',
  online: 'bg-status-up',
  offline: 'bg-status-down',
  revoked: 'bg-status-none',
}

/** The command that starts an agent for a new probe, as the operator runs it on the other machine. */
export const probeCommand = (origin: string, token: string): string =>
  `v1per-probe --server ${origin.replace(/\/+$/, '')} --token ${token}`

/** What holds a probe back, most serious first; empty when it keeps up. */
export type ProbeLag =
  | { kind: 'dropped'; count: number }
  | { kind: 'buffered'; count: number }
  | { kind: 'late'; late: number; started: number }

export function probeLag(probe: Pick<Probe, 'dropped_results' | 'buffered_results' | 'checks_late' | 'checks_started'>): ProbeLag[] {
  const lag: ProbeLag[] = []
  if (probe.dropped_results > 0) lag.push({ kind: 'dropped', count: probe.dropped_results })
  if (probe.buffered_results > 0) lag.push({ kind: 'buffered', count: probe.buffered_results })
  if (probe.checks_late > 0) lag.push({ kind: 'late', late: probe.checks_late, started: probe.checks_started })
  return lag
}
