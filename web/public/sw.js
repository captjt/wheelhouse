// Wheelhouse's service worker: shows pushes from the hub, and opens the
// docket on a tap. It caches nothing of the app: the app is always the
// hub's own. It takes images shared to Wheelhouse from the phone's share
// sheet (the manifest's share_target): they wait in a cache of their own
// until the Share page (#/share) picks them up and clears it.
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

self.addEventListener("push", (e) => {
  let d = {};
  try {
    d = e.data ? e.data.json() : {};
  } catch {
    d = { body: e.data && e.data.text() };
  }
  e.waitUntil(
    self.registration.showNotification(d.title || "Wheelhouse", {
      body: d.body || "",
      tag: d.tag,
      data: { url: d.url || "/" },
      icon: "/icon-192.png",
      badge: "/icon-192.png",
    }),
  );
});

self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const url = (e.notification.data && e.notification.data.url) || "/";
  e.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const w of wins) {
        w.postMessage({ open: url });
        return w.focus();
      }
      return self.clients.openWindow(url);
    })(),
  );
});

// Shared from another app: the images (and any words) kept for the Share
// page, which reads them with the app's own headers, then opens it. Nothing
// is sent to the hub from here.
const SHARE_CACHE = "wh-share";
const MAX_SHARED = 10;
const SENDABLE = /^image\/(png|jpeg|gif|webp|heic|heif|avif)$/;
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "POST" || url.origin !== self.location.origin || url.pathname !== "/share-target") return;
  // From the share sheet (the browser's own, "none") or this app; never a
  // form another site posts here.
  const from = e.request.headers.get("Sec-Fetch-Site");
  const ok = !from || from === "none" || from === "same-origin";
  e.respondWith(
    (async () => {
      if (!ok) return Response.redirect("/#/share", 303);
      try {
        const form = await e.request.formData();
        const c = await caches.open(SHARE_CACHE);
        for (const k of await c.keys()) await c.delete(k);
        let n = 0;
        for (const f of form.getAll("images")) {
          if (typeof f === "string" || !SENDABLE.test(f.type) || n >= MAX_SHARED) continue;
          await c.put(`/share-inbox/file/${n++}`, new Response(f, { headers: { "content-type": f.type, "x-name": encodeURIComponent(f.name || "shared") } }));
        }
        const words = [form.get("title"), form.get("text"), form.get("url")].filter((x) => typeof x === "string" && x.trim()).join("\n");
        if (words) await c.put("/share-inbox/text", new Response(words.slice(0, 4000)));
      } catch {
        // The Share page says nothing came.
      }
      return Response.redirect("/#/share", 303);
    })(),
  );
});
