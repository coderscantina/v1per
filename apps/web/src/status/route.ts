import type { MessageKey } from './messages'

/** The page's views, each at its own path below the page's base, so a view's link can be shared. */
export const views = ['status', 'maintenance', 'incidents'] as const
export type PageRoute = (typeof views)[number]

export const viewLabel: Record<PageRoute, MessageKey> = {
  status: 'nav.status',
  maintenance: 'nav.maintenance',
  incidents: 'nav.incidents',
}

/** The view at `pathname`; any other path below the base, such as a share link, is the status view. */
function viewAt(pathname: string, base: string): PageRoute {
  const rest = pathname.slice(base.length).replace(/^\/|\/$/g, '')
  if (incidentAt(pathname, base) !== null) return 'incidents'
  return rest === 'maintenance' || rest === 'incidents' ? rest : 'status'
}

/** The incident `<base>/incidents/<id>` names, or null. */
function incidentAt(pathname: string, base: string): string | null {
  return /^\/incidents\/(\d+)\/?$/.exec(pathname.slice(base.length))?.[1] ?? null
}

/** Leaves modified clicks (new tab, new window) to the browser. */
function plainClick(event: MouseEvent): boolean {
  return !(event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0)
}

/**
 * The view the address names, and the incident on `<base>/incidents/<id>`, switched in place with
 * the History API. The page is one document, so switching keeps the live snapshot and needs no
 * request. An incident's page belongs to the incidents view.
 */
export function usePageRoute(base: string) {
  const current = ref(viewAt(window.location.pathname, base))
  const incident = ref(incidentAt(window.location.pathname, base))

  function href(view: PageRoute): string {
    const path = view === 'status' ? base || '/' : `${base}/${view}`
    return `${path}${window.location.search}`
  }

  function incidentHref(id: string): string {
    return `${base}/incidents/${id}${window.location.search}`
  }

  function go(path: string, event: MouseEvent) {
    event.preventDefault()
    window.history.pushState(null, '', path)
    sync()
    window.scrollTo({ top: 0 })
  }

  /** Follows a click on a view link. */
  function open(view: PageRoute, event: MouseEvent) {
    if (!plainClick(event)) return
    if (view === current.value && incident.value === null) return event.preventDefault()
    go(href(view), event)
  }

  /** Follows a click on an incident's link. */
  function openIncident(id: string, event: MouseEvent) {
    if (plainClick(event)) go(incidentHref(id), event)
  }

  function sync() {
    current.value = viewAt(window.location.pathname, base)
    incident.value = incidentAt(window.location.pathname, base)
  }
  onMounted(() => window.addEventListener('popstate', sync))
  onBeforeUnmount(() => window.removeEventListener('popstate', sync))

  return { current, incident, href, incidentHref, open, openIncident }
}

export type PageRouter = ReturnType<typeof usePageRoute>
