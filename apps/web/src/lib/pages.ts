import type {
  Accent,
  AutoIncidentMode,
  FieldErrorCode,
  Locale,
  Page,
  PageAccess,
  PageStatus,
  PageTheme,
  PagesUpdateAccessArgs,
  PagesUpdateArgs,
} from '~/generated'
import { PAGE_ALLOWED_EMAILS_MAX, PAGE_GRACE_MAX_MINUTES, PAGE_SLUG_MAX_CHARS } from '~/generated/limits'
import type { MessageKey } from '~/plugins/i18n'

/** Status page settings, access and look (decision 0004, status page API). */

export const themes = ['auto', 'light', 'dark'] as const satisfies readonly PageTheme[]
export const accents = ['indigo', 'blue', 'violet', 'rose', 'orange', 'amber', 'emerald', 'cyan'] as const satisfies readonly Accent[]
export const pageLocales = ['en', 'de'] as const satisfies readonly Locale[]
export const autoIncidentModes = ['draft', 'auto_publish'] as const satisfies readonly AutoIncidentMode[]

/** Language names in their own language, as visitors pick them. */
export const localeNames: Record<Locale, string> = { en: 'English', de: 'Deutsch' }

type FormErrors = Partial<Record<string, { code: FieldErrorCode; limit?: number }>>

const SLUG = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/

/** A slug suggestion from a title: "Acme Cloud Status" becomes `acme-cloud-status`. */
export function slugify(title: string): string {
  return title
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .slice(0, PAGE_SLUG_MAX_CHARS)
    .replace(/^-+|-+$/g, '')
}

export function validateSlug(slug: string): FormErrors {
  if (slug === '') return { slug: { code: 'required' } }
  if (slug.length > PAGE_SLUG_MAX_CHARS) return { slug: { code: 'too_long', limit: PAGE_SLUG_MAX_CHARS } }
  return SLUG.test(slug) ? {} : { slug: { code: 'invalid_format' } }
}

export interface SettingsForm {
  title: string
  slug: string
  description: string
  footer: string
  theme: PageTheme
  accent: Accent
  /** Empty: the instance timezone. */
  timezone: string
  defaultLocale: Locale
  locales: Locale[]
  autoIncidents: AutoIncidentMode
  graceMinutes: string
}

export function settingsFromPage(page: Page): SettingsForm {
  const defaultLocale = page.default_locale ?? 'en'
  return {
    title: page.title,
    slug: page.slug,
    description: page.description ?? '',
    footer: page.footer ?? '',
    theme: page.theme ?? 'auto',
    accent: page.accent ?? 'indigo',
    timezone: page.timezone ?? '',
    defaultLocale,
    locales: page.locales ?? [defaultLocale],
    autoIncidents: page.auto_incidents ?? 'draft',
    graceMinutes: String(page.grace_minutes ?? 0),
  }
}

export function validateSettings(form: SettingsForm): FormErrors {
  const errors: FormErrors = { ...validateSlug(form.slug.trim()) }
  if (form.title.trim() === '') errors.title = { code: 'required' }
  const grace = Number(form.graceMinutes)
  if (form.autoIncidents === 'auto_publish' && (!Number.isInteger(grace) || grace < 0 || grace > PAGE_GRACE_MAX_MINUTES)) {
    errors.grace_minutes = { code: 'invalid_value' }
  }
  return errors
}

/** Every field is sent: an absent one would reset it to the default. `basedOn` is the revision the form was loaded at. */
export function toUpdateArgs(form: SettingsForm, page: Page, basedOn: number): PagesUpdateArgs {
  return {
    page: page.id,
    expected_revision: basedOn,
    title: form.title.trim(),
    slug: form.slug.trim(),
    description: form.description.trim() || undefined,
    footer: form.footer.trim() || undefined,
    theme: form.theme,
    accent: form.accent,
    timezone: form.timezone.trim() || undefined,
    default_locale: form.defaultLocale,
    // The default is always enabled; keep it first, as visitors see the list.
    locales: [form.defaultLocale, ...pageLocales.filter((code) => code !== form.defaultLocale && form.locales.includes(code))],
    auto_incidents: form.autoIncidents,
    grace_minutes: form.autoIncidents === 'auto_publish' ? Number(form.graceMinutes) : (page.grace_minutes ?? 0),
  }
}

/** What happens to the stored page password on save. */
export type PasswordAction = 'keep' | 'set' | 'clear'

export interface AccessForm {
  public: boolean
  passwordAction: PasswordAction
  password: string
  /** One address or domain per line; commas and spaces separate too. */
  emails: string
  domains: string
}

export function accessFromPage(access: PageAccess): AccessForm {
  return {
    public: access.public,
    passwordAction: 'keep',
    password: '',
    emails: (access.emails ?? []).join('\n'),
    domains: (access.domains ?? []).join('\n'),
  }
}

/** Entries of a typed list, lowercase, without blanks and repeats, in the order typed. */
export const parseList = (text: string): string[] => [
  ...new Set(
    text
      .split(/[\s,;]+/)
      .map((entry) => entry.trim().toLowerCase())
      .filter((entry) => entry !== '')
  ),
]

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
const DOMAIN = /^(?!-)[a-z0-9-]{1,63}(?<!-)(?:\.(?!-)[a-z0-9-]{1,63}(?<!-))+$/

export function validateAccess(form: AccessForm): FormErrors {
  const errors: FormErrors = {}
  if (form.passwordAction === 'set' && form.password === '') errors.password = { code: 'required' }
  if (form.passwordAction === 'set' && form.password.length > 256) errors.password = { code: 'too_long', limit: 256 }
  const emails = parseList(form.emails)
  if (emails.length > PAGE_ALLOWED_EMAILS_MAX) errors.emails = { code: 'too_many', limit: PAGE_ALLOWED_EMAILS_MAX }
  else if (!emails.every((email) => EMAIL.test(email))) errors.emails = { code: 'invalid_format' }
  const domains = parseList(form.domains).map((domain) => domain.replace(/^@/, ''))
  if (!domains.every((domain) => DOMAIN.test(domain))) errors.domains = { code: 'invalid_format' }
  return errors
}

/** A password is sent only when it changes; `set` in a request is ignored by the server. */
export function toAccessArgs(form: AccessForm, page: Page, basedOn: number): PagesUpdateAccessArgs {
  const password =
    form.passwordAction === 'set'
      ? { value: form.password, set: true }
      : form.passwordAction === 'clear'
        ? { clear: true, set: false }
        : undefined
  return {
    page: page.id,
    expected_revision: basedOn,
    public: form.public,
    password,
    emails: parseList(form.emails),
    domains: parseList(form.domains).map((domain) => domain.replace(/^@/, '')),
  }
}

/** How visitors get in besides grant holders, who always do. */
export type AccessMode = 'public' | 'password' | 'email' | 'tokens' | 'team'

export function accessModes(access: PageAccess, shareTokens: number): AccessMode[] {
  if (access.public) return ['public']
  const modes: AccessMode[] = []
  if (access.password?.set) modes.push('password')
  if ((access.emails?.length ?? 0) > 0 || (access.domains?.length ?? 0) > 0) modes.push('email')
  if (shareTokens > 0) modes.push('tokens')
  return modes.length > 0 ? modes : ['team']
}

export const accessModeLabels: Record<AccessMode, MessageKey> = {
  public: 'pages.access.modes.public',
  password: 'pages.access.modes.password',
  email: 'pages.access.modes.email',
  tokens: 'pages.access.modes.tokens',
  team: 'pages.access.modes.team',
}

export const pageStatusLabels: Record<PageStatus, MessageKey> = {
  operational: 'pages.statuses.operational',
  maintenance: 'pages.statuses.maintenance',
  degraded: 'pages.statuses.degraded',
  partial_outage: 'pages.statuses.partial_outage',
  major_outage: 'pages.statuses.major_outage',
}

/** The dot colour. A word always goes with it. */
export const pageStatusFill: Record<PageStatus, string> = {
  operational: 'bg-status-up',
  maintenance: 'bg-status-maintenance',
  degraded: 'bg-status-pending',
  partial_outage: 'bg-status-partial',
  major_outage: 'bg-status-down',
}

/** The page's address on this instance, for links and share links. */
export const pageUrl = (page: Pick<Page, 'path'>, origin = window.location.origin) => `${origin}${page.path}`
