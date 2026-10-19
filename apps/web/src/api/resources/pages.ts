import type {
  IncidentId,
  PageDetail,
  PageId,
  PagesCreateArgs,
  PagesCreateShareTokenArgs,
  PagesListArgs,
  PagesPreviewArgs,
  PagesUpdateAccessArgs,
  PagesUpdateArgs,
  PagesUpdateLayoutArgs,
  ShareTokenId,
} from '~/generated'

import { api } from '..'

/**
 * Status pages. `list` and `get` follow the node's `pages:<id>` topic; `preview` reads the page
 * as one audience sees it, from the page's own `page:<id>` topic.
 */
export const pagesResource = {
  list: (args: PagesListArgs, signal?: AbortSignal) => api.query('pages.list', args, { signal }),
  get: (page: PageId, signal?: AbortSignal) => api.query('pages.get', { page }, { signal }),
  preview: (args: PagesPreviewArgs, signal?: AbortSignal) => api.query('pages.preview', args, { signal }),
  create: (args: PagesCreateArgs) => api.command('pages.create', args),
  update: (args: PagesUpdateArgs) => api.command('pages.update', args),
  updateAccess: (args: PagesUpdateAccessArgs) => api.command('pages.updateAccess', args),
  updateLayout: (args: PagesUpdateLayoutArgs) => api.command('pages.updateLayout', args),
  claimHostname: (page: PageId, hostname: string) => api.command('pages.claimHostname', { page, hostname }),
  releaseHostname: (page: PageId, hostname: string) => api.command('pages.releaseHostname', { page, hostname }),
  /** The image as the raw body; the server recognizes PNG, JPEG, GIF, WebP and SVG by their bytes. */
  uploadLogo: (page: PageId, image: Blob) => api.upload<PageDetail>(`/pages/${page}/logo`, image),
  removeLogo: (page: PageId) => api.command('pages.removeLogo', { page }),
  delete: (page: PageId) => api.command('pages.delete', { page }),
  createShareToken: (args: PagesCreateShareTokenArgs) => api.command('pages.createShareToken', args),
  revokeShareToken: (token: ShareTokenId) => api.command('pages.revokeShareToken', { token }),
  /** The pages an incident is on or could go on. */
  publications: (incident: IncidentId, signal?: AbortSignal) => api.query('incidents.publications', { incident }, { signal }),
  publish: (incident: IncidentId, page: PageId) => api.command('incidents.publish', { incident, page }),
  unpublish: (incident: IncidentId, page: PageId) => api.command('incidents.unpublish', { incident, page }),
}
