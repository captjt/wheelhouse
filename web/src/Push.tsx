import { useEffect, useState } from "react";
import { get, post } from "./api";
import { IconBell } from "./icons";
import { go } from "./live";
import { onThisMac } from "./Pane";
import { useToast } from "./ui";

let registration: ServiceWorkerRegistration | null = null;

/** Register the service worker, and follow its "open this" from a tapped push. */
export function registerWorker() {
  if (!("serviceWorker" in navigator) || !window.isSecureContext) return;
  navigator.serviceWorker.register("/sw.js").then(
    (r) => (registration = r),
    () => {},
  );
  navigator.serviceWorker.addEventListener("message", (e) => {
    const url: string | undefined = e.data?.open;
    if (url) go(url.replace(/^\/?#?/, "").replace(/^\/?/, "/"));
  });
}

const b64url = (s: string) => {
  const pad = "=".repeat((4 - (s.length % 4)) % 4);
  const raw = atob((s + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
};

const standalone = () => window.matchMedia("(display-mode: standalone)").matches || (navigator as unknown as { standalone?: boolean }).standalone === true;
const ios = () => /iPhone|iPad|iPod/.test(navigator.userAgent);

/** Off the Mac: one line to turn on phone notifications, until they're on. */
export function PushHint() {
  const toast = useToast();
  const [state, setState] = useState<"unknown" | "off" | "on" | "denied" | "unsupported">("unknown");
  useEffect(() => {
    if (onThisMac()) return setState("on"); // the Mac has its own notifications
    if (!("serviceWorker" in navigator) || !("PushManager" in window) || !window.isSecureContext) {
      // iOS offers push only to the Home Screen app.
      return setState(ios() && !standalone() ? "off" : "unsupported");
    }
    if (Notification.permission === "denied") return setState("denied");
    navigator.serviceWorker.ready.then(async (r) => {
      const sub = await r.pushManager.getSubscription();
      setState(sub && Notification.permission === "granted" ? "on" : "off");
      // Keep the hub's copy of a live subscription current.
      if (sub && Notification.permission === "granted") post("/api/push/subscribe", { subscription: sub.toJSON(), agent: navigator.userAgent }).catch(() => {});
    });
  }, []);
  if (state === "on" || state === "unknown" || state === "unsupported") return null;
  const turnOn = async () => {
    if (ios() && !standalone()) {
      toast("On iPhone, add Wheelhouse to your Home Screen first (Share › Add to Home Screen), then open it from there.");
      return;
    }
    try {
      const perm = await Notification.requestPermission();
      if (perm !== "granted") return setState(perm === "denied" ? "denied" : "off");
      const r = registration ?? (await navigator.serviceWorker.ready);
      const { public_key } = await get<{ public_key: string }>("/api/push/key");
      const sub = await r.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64url(public_key) });
      await post("/api/push/subscribe", { subscription: sub.toJSON(), agent: navigator.userAgent });
      const t = await post<{ sent: number }>("/api/push/test");
      setState("on");
      toast(t.sent ? "Notifications are on" : "Subscribed, but the test didn't send", t.sent ? undefined : "error");
    } catch (e) {
      toast(`Couldn't turn on notifications: ${(e as Error).message}`, "error");
    }
  };
  return (
    <button className="hint" onClick={turnOn} title="Push notifications on this device, for what needs you">
      <IconBell size={14} />
      <span className="hint-title">{state === "denied" ? "Notifications blocked" : "Notifications off"}</span>
      <span className="spacer" />
      <span className="hint-action">{state === "denied" ? "Settings" : "Turn on"}</span>
    </button>
  );
}
