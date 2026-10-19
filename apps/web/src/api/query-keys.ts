/**
 * Vue Query keys start with the operation name and include every argument. Restricted results
 * are dropped from the cache on sign-in, sign-out and any `unauthenticated` answer. Node-scoped
 * lists, pickers and overview summaries are live snapshots (`useLiveSnapshot`), not cached here.
 */
export const queryKeys = {
  session: ['auth.session'] as const,
  twoFactor: ['twoFactor.status'] as const,
  preferences: ['account.preferences'] as const,
  apiKeys: ['apiKeys.list'] as const,
  instanceSettings: ['instance.settings'] as const,
  instanceInfo: ['instance.info'] as const,
  imports: ['imports.list'] as const,
  importRun: (node: string) => ['imports.get', node] as const,
  importInventory: (node: string) => ['imports.inventory', node] as const,
  invitation: (code: string) => ['invitations.preview', code] as const,
  /** One-off member reads, such as the move preview; member pages follow the live snapshot. */
  members: (node: string) => ['members.list', node] as const,
  /** Everything read about one monitor's history, so a status change refreshes it in one call. */
  monitorHistory: (monitor: string) => ['monitor-history', monitor] as const,
  /** `probe` is empty for the monitor's own status, else one probe's results. */
  uptime: (monitor: string, probe: string) => ['monitor-history', monitor, 'monitors.uptime', probe] as const,
  latency: (monitor: string, range: string, probe: string) =>
    ['monitor-history', monitor, 'monitors.latency', range, probe] as const,
  dayBars: (monitor: string, timezone: string) => ['monitor-history', monitor, 'monitors.dayBars', timezone] as const,
  heartbeats: (monitor: string, probe: string) => ['monitor-history', monitor, 'monitors.heartbeats', probe] as const,
  /** The hourly trend of a node and everything below it; read it again when a status changes. */
  trend: (node: string) => ['monitors.trend', node] as const,
  /** Each monitor's last 24 hours on a node and everything below it; read it again when a status changes. */
  hourly: (node: string) => ['monitors.hourly', node] as const,
  /** A monitor's recent alerts, refreshed with its history. */
  monitorDeliveries: (monitor: string) => ['monitor-history', monitor, 'channels.monitorDeliveries'] as const,
  /** One delivery, such as a test send, read until it finishes. */
  delivery: (delivery: string) => ['channels.delivery', delivery] as const,
  /** A page as one audience sees it, read when the preview opens or is refreshed. */
  pagePreview: (page: string, audience: string, locale: string) => ['pages.preview', page, audience, locale] as const,
  /** The pages an incident is on, read again when the incident changes. */
  publications: (incident: string) => ['incidents.publications', incident] as const,
  /** The next windows of an unsaved schedule, keyed by the schedule itself. */
  maintenancePreview: (schedule: string) => ['maintenance.preview', schedule] as const,
}
