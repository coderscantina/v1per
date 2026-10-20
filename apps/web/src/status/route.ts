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
  return rest === 'maintenance' || rest === 'incidents' ? rest : 'status'
}

/**
 * The view the address names, switched in place with the History API. The page is one document,
 * so switching keeps the live snapshot and needs no request.
 */
export function usePageRoute(base: string) {
  const current = ref(viewAt(window.location.pathname, base))

  function href(view: PageRoute): string {
    const path = view === 'status' ? base || '/' : `${base}/${view}`
    return `${path}${window.location.search}`
  }

  /** Follows a click on a view link, leaving modified clicks (new tab, new window) to the browser. */
  function open(view: PageRoute, event: MouseEvent) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) return
    event.preventDefault()
    if (view === current.value) return
    window.history.pushState(null, '', href(view))
    current.value = view
    window.scrollTo({ top: 0 })
  }

  const sync = () => (current.value = viewAt(window.location.pathname, base))
  onMounted(() => window.addEventListener('popstate', sync))
  onBeforeUnmount(() => window.removeEventListener('popstate', sync))

  return { current, href, open }
}
