// tasq web: keeps the page itself for opening offline. Your calendar
// (/v1/…) is never stored here: it's asked for fresh each time.
const CACHE = 'tasq-web-v1';
const SHELL = ['/', '/app.js', '/app.css', '/themes.json', '/manifest.webmanifest', '/icon.svg', '/apple-touch-icon.png'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).catch(() => {}).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (e) => {
  e.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
    .then(() => self.clients.claim()));
});

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET' || url.origin !== location.origin || !SHELL.includes(url.pathname)) return;
  // The network first, so an update shows at once; the copy when offline.
  e.respondWith(fetch(e.request).then((res) => {
    if (res.ok && !res.redirected) {
      const copy = res.clone();
      caches.open(CACHE).then((c) => c.put(e.request, copy));
    }
    return res;
  }).catch(() => caches.match(e.request)));
});
