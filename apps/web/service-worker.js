// The admin app's service worker. `vite.config.ts` builds it into `dist/sw.js` with the files to
// precache, so the installed app opens offline. API calls, `/config.json` and status pages always
// go to the network.
const CACHE = 'v1per-__VERSION__'
const PRECACHE = __PRECACHE__

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE))
      .then(() => self.skipWaiting())
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim())
  )
})

self.addEventListener('fetch', (event) => {
  const { request } = event
  const url = new URL(request.url)
  if (request.method !== 'GET' || url.origin !== self.location.origin) return

  if (request.mode === 'navigate') {
    if (url.pathname.startsWith('/s/') || url.pathname.startsWith('/api/')) return
    // The network first, so a deploy shows at the next load. Offline, the cached shell.
    event.respondWith(
      fetch(request).catch(() => caches.match('/index.html').then((shell) => shell ?? Response.error()))
    )
  } else if (url.pathname.startsWith('/assets/')) {
    // Hashed file names never change, so a cached copy is always current.
    event.respondWith(caches.match(request).then((hit) => hit ?? fetch(request)))
  }
})
