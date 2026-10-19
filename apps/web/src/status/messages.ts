import { ref } from 'vue'

import type { Locale } from '~/generated/Locale'

/**
 * The status page's own copy, kept out of the admin catalogs so the page does not load them.
 * German must carry every English key; the type says so.
 */
const en = {
  'status.operational': 'Operational',
  'status.maintenance': 'Maintenance',
  'status.degraded': 'Degraded',
  'status.partial_outage': 'Partial outage',
  'status.major_outage': 'Major outage',
  'status.none': 'No data',

  'banner.operational': 'All systems operational',
  'banner.maintenance': 'Maintenance in progress',
  'banner.degraded': 'Degraded performance',
  'banner.partial_outage': 'Partial outage',
  'banner.major_outage': 'Major outage',
  'banner.updated': 'Updated {time}',


  'state.investigating': 'Investigating',
  'state.identified': 'Identified',
  'state.monitoring': 'Monitoring',
  'state.resolved': 'Resolved',

  'incidents.active': 'Active incidents',
  'incidents.affected': 'Affected',
  'incidents.timeline': 'Show all updates',
  'incidents.hideTimeline': 'Hide updates',
  'incidents.history': 'Past incidents',
  'incidents.none': 'No incidents in the last 90 days.',
  'incidents.more': 'Show more',
  'incidents.loading': 'Loading…',
  'incidents.error': 'Past incidents could not be loaded.',
  'incidents.retry': 'Try again',
  'incidents.resolvedAfter': 'Resolved after {duration}',
  'incidents.started': 'Started {time}',

  'maintenance.title': 'Maintenance',
  'maintenance.active': 'In progress',
  'maintenance.scheduled': 'Scheduled',
  'maintenance.window': '{start} to {end}',
  'maintenance.from': 'From {start}',
  'maintenance.until': 'Until {end}',

  'components.title': 'Components',
  'components.daysAgo': '90 days ago',
  'components.today': 'Today',
  'components.uptime': '{value} uptime',
  'components.history': 'Last 90 days of {name}',
  'components.toggle': 'Show or hide {name}',

  'bar.uptime': '{value} uptime',
  'bar.noData': 'No data',
  'bar.maintenance': 'Maintenance this day',
  'bar.approximate': 'Imported history, counted by UTC day',
  'bar.incidents': 'Incidents',
  'bar.label': '{date}: {status}',
  'bar.labelUptime': '{date}: {status}, {uptime} uptime',

  'internal.badge': 'Team view',
  'internal.hint': 'You see internal components and response times because you are signed in.',
  'internal.responseTime': 'Response time, last 24 hours',
  'internal.latencySummary': 'Average {avg}, fastest {min}, slowest {max} over the last 24 hours',
  'internal.noLatency': 'No response times in the last 24 hours.',
  'internal.probes': 'Probes',
  'internal.checked': 'checked {time}',
  'probe.up': 'Up',
  'probe.down': 'Down',
  'probe.pending': 'Pending',
  'probe.maintenance': 'Maintenance',

  'gate.title': 'This page is private',
  'gate.lead': 'Sign in to see this status page.',
  'gate.password': 'Password',
  'gate.passwordSubmit': 'Open page',
  'gate.email': 'Email',
  'gate.emailLead': 'Get a sign-in link by email.',
  'gate.emailSubmit': 'Send link',
  'gate.emailSent': 'If this address may open the page, a link is on its way. It works once, for 15 minutes.',
  'gate.or': 'or',
  'gate.team': 'Team member?',
  'gate.signIn': 'Sign in',
  'gate.closed': 'Only team members can open this page.',
  'gate.redeeming': 'Signing you in…',
  'gate.continue': 'Continue',
  'gate.confirm': 'Continue to open this page with your sign-in link.',
  'gate.working': 'Checking…',

  'error.password': 'Wrong password.',
  'error.code': 'This link has expired or was already used. Request a new one.',
  'error.rateLimited': 'Too many attempts. Wait a minute and try again.',
  'error.email': 'Enter a valid email address.',
  'error.origin': 'Open this page at its own address and try again.',
  'error.network': 'Could not reach the server. Check your connection and try again.',
  'error.unknown': 'Something went wrong. Try again.',
  'error.boot': 'This status page could not load.',

  'footer.language': 'Language',
  'footer.poweredBy': 'Powered by v1per',

  'duration.minutes': '{n} min',
  'duration.hours': '{h} h {m} min',
  'duration.days': '{d} d {h} h',
} as const

type MessageKey = keyof typeof en

const de: Record<MessageKey, string> = {
  'status.operational': 'Betriebsbereit',
  'status.maintenance': 'Wartung',
  'status.degraded': 'Eingeschränkt',
  'status.partial_outage': 'Teilausfall',
  'status.major_outage': 'Ausfall',
  'status.none': 'Keine Daten',

  'banner.operational': 'Alle Systeme betriebsbereit',
  'banner.maintenance': 'Wartung läuft',
  'banner.degraded': 'Eingeschränkte Leistung',
  'banner.partial_outage': 'Teilausfall',
  'banner.major_outage': 'Größerer Ausfall',
  'banner.updated': 'Aktualisiert {time}',


  'state.investigating': 'Wird untersucht',
  'state.identified': 'Ursache gefunden',
  'state.monitoring': 'Wird beobachtet',
  'state.resolved': 'Behoben',

  'incidents.active': 'Aktuelle Störungen',
  'incidents.affected': 'Betroffen',
  'incidents.timeline': 'Alle Meldungen zeigen',
  'incidents.hideTimeline': 'Meldungen ausblenden',
  'incidents.history': 'Vergangene Störungen',
  'incidents.none': 'Keine Störungen in den letzten 90 Tagen.',
  'incidents.more': 'Mehr zeigen',
  'incidents.loading': 'Wird geladen…',
  'incidents.error': 'Vergangene Störungen konnten nicht geladen werden.',
  'incidents.retry': 'Erneut versuchen',
  'incidents.resolvedAfter': 'Behoben nach {duration}',
  'incidents.started': 'Begonnen {time}',

  'maintenance.title': 'Wartung',
  'maintenance.active': 'Läuft',
  'maintenance.scheduled': 'Geplant',
  'maintenance.window': '{start} bis {end}',
  'maintenance.from': 'Ab {start}',
  'maintenance.until': 'Bis {end}',

  'components.title': 'Komponenten',
  'components.daysAgo': 'vor 90 Tagen',
  'components.today': 'Heute',
  'components.uptime': '{value} verfügbar',
  'components.history': 'Letzte 90 Tage von {name}',
  'components.toggle': '{name} ein- oder ausblenden',

  'bar.uptime': '{value} verfügbar',
  'bar.noData': 'Keine Daten',
  'bar.maintenance': 'Wartung an diesem Tag',
  'bar.approximate': 'Importierter Verlauf, nach UTC-Tag gezählt',
  'bar.incidents': 'Störungen',
  'bar.label': '{date}: {status}',
  'bar.labelUptime': '{date}: {status}, {uptime} verfügbar',

  'internal.badge': 'Teamansicht',
  'internal.hint': 'Du siehst interne Komponenten und Antwortzeiten, weil du angemeldet bist.',
  'internal.responseTime': 'Antwortzeit, letzte 24 Stunden',
  'internal.latencySummary': 'Durchschnitt {avg}, schnellste {min}, langsamste {max} in den letzten 24 Stunden',
  'internal.noLatency': 'Keine Antwortzeiten in den letzten 24 Stunden.',
  'internal.probes': 'Prüfstellen',
  'internal.checked': 'geprüft {time}',
  'probe.up': 'Erreichbar',
  'probe.down': 'Ausgefallen',
  'probe.pending': 'Ausstehend',
  'probe.maintenance': 'Wartung',

  'gate.title': 'Diese Seite ist privat',
  'gate.lead': 'Melde dich an, um diese Statusseite zu sehen.',
  'gate.password': 'Passwort',
  'gate.passwordSubmit': 'Seite öffnen',
  'gate.email': 'E-Mail',
  'gate.emailLead': 'Erhalte einen Anmeldelink per E-Mail.',
  'gate.emailSubmit': 'Link senden',
  'gate.emailSent': 'Wenn diese Adresse die Seite öffnen darf, ist ein Link unterwegs. Er gilt einmal, 15 Minuten lang.',
  'gate.or': 'oder',
  'gate.team': 'Im Team?',
  'gate.signIn': 'Anmelden',
  'gate.closed': 'Nur Teammitglieder können diese Seite öffnen.',
  'gate.redeeming': 'Du wirst angemeldet…',
  'gate.continue': 'Weiter',
  'gate.confirm': 'Weiter, um diese Seite mit deinem Anmeldelink zu öffnen.',
  'gate.working': 'Wird geprüft…',

  'error.password': 'Falsches Passwort.',
  'error.code': 'Dieser Link ist abgelaufen oder wurde schon benutzt. Fordere einen neuen an.',
  'error.rateLimited': 'Zu viele Versuche. Warte eine Minute und versuche es erneut.',
  'error.email': 'Gib eine gültige E-Mail-Adresse ein.',
  'error.origin': 'Öffne diese Seite unter ihrer eigenen Adresse und versuche es erneut.',
  'error.network': 'Der Server ist nicht erreichbar. Prüfe deine Verbindung und versuche es erneut.',
  'error.unknown': 'Etwas ist schiefgegangen. Versuche es erneut.',
  'error.boot': 'Diese Statusseite konnte nicht geladen werden.',

  'footer.language': 'Sprache',
  'footer.poweredBy': 'Betrieben mit v1per',

  'duration.minutes': '{n} Min.',
  'duration.hours': '{h} Std. {m} Min.',
  'duration.days': '{d} T. {h} Std.',
}

const catalogs: Record<Locale, Record<MessageKey, string>> = { en, de }

export type { MessageKey }

/** The page's locale: the snapshot's or the gate's, which the server picked among the enabled ones. */
export const locale = ref<Locale>('en')

/** The text for `key` in the current locale, with `{name}` placeholders filled. */
export function t(key: MessageKey, params: Record<string, string | number> = {}): string {
  const text = catalogs[locale.value][key]
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match))
}
