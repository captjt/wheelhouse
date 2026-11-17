import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { get, post, type Docket, type HubState } from "./api";
import { useToast } from "./ui";

// One stream of the hub's events for the whole app. Each event bumps a
// version; data fetches again when it moves.
const Version = createContext(0);

export function LiveProvider({ children }: { children: ReactNode }) {
  const [version, setVersion] = useState(0);
  useEffect(() => {
    let timer: number | undefined;
    let es: EventSource | null = null;
    let retry: number | undefined;
    const bump = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setVersion((v) => v + 1), 100);
    };
    const connect = () => {
      es = new EventSource("/api/events");
      es.onmessage = bump;
      es.onopen = bump;
      es.onerror = () => {
        es?.close();
        retry = window.setTimeout(connect, 2000);
      };
    };
    connect();
    // Ages ("4m") move on even when nothing happens.
    const tick = window.setInterval(() => setVersion((v) => v + 1), 30000);
    return () => {
      es?.close();
      window.clearTimeout(timer);
      window.clearTimeout(retry);
      window.clearInterval(tick);
    };
  }, []);
  return <Version.Provider value={version}>{children}</Version.Provider>;
}

export function useData<T>(path: string | null): { data: T | null; error: string | null; reload: () => void } {
  const version = useContext(Version);
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    if (!path) return;
    let live = true;
    get<T>(path)
      .then((d) => {
        if (live) {
          setData(d);
          setError(null);
        }
      })
      .catch((e: Error) => live && setError(e.message));
    return () => {
      live = false;
    };
  }, [path, version, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data, error, reload };
}

// ---- the hub's state, and what you can do to it ----

type Hub = {
  state: HubState | null;
  error: string | null;
  reload: () => void;
  byId: Map<string, Docket>;
  setStatus: (id: string, status: Docket["status"]) => Promise<void>;
  rename: (id: string, title: string) => Promise<void>;
  note: (id: string, body: string) => Promise<void>;
  openInRex: (id: string) => Promise<void>;
  answer: (d: Docket, approve: boolean) => Promise<void>;
  create: (title: string, note?: string) => Promise<string>;
};

const HubCtx = createContext<Hub | null>(null);

export function HubProvider({ children }: { children: ReactNode }) {
  const { data, error, reload } = useData<HubState>("/api/state");
  const toast = useToast();
  const byId = useMemo(() => new Map((data?.dockets ?? []).map((d) => [d.id, d])), [data]);
  const words = { todo: "To do", in_progress: "In progress", done: "Done", dropped: "Dropped" };
  const fail = (e: Error) => {
    toast(e.message, "error");
    throw e;
  };
  const hub: Hub = {
    state: data,
    error,
    reload,
    byId,
    setStatus: (id, status) =>
      post(`/api/dockets/${id}/set`, { status }).then(() => {
        toast(`${id} moved to ${words[status]}`);
        reload();
      }, fail),
    rename: (id, title) => post(`/api/dockets/${id}/set`, { title }).then(reload, fail),
    note: (id, body) => post(`/api/dockets/${id}/note`, { body }).then(reload, fail),
    openInRex: (id) =>
      post(`/api/dockets/${id}/open`).then(
        () => toast(`Showing ${id}'s pane in Rex`),
        (e: Error) => toast(e.message, "error"),
      ),
    answer: (d, approve) => {
      const l = d.live;
      if (!l) return Promise.resolve();
      return post<{ cleared: boolean; now: string }>(`/api/dockets/${d.id}/${approve ? "approve" : "deny"}`, {
        record: l.record ?? "",
        updated_at: l.since ?? null,
        msg: l.msg ?? null,
      }).then(
        (r) => {
          toast(r.cleared ? `${approve ? "Approved" : "Denied"}${l.msg ? `: ${l.msg}` : ""}` : "Sent, but the prompt is still showing. Look at the pane.", r.cleared ? undefined : "error");
          reload();
        },
        (e: Error) => {
          toast(e.message, "error");
          reload();
        },
      );
    },
    create: async (title, note) => {
      const r = await post<{ docket: string }>("/api/dockets", { title }).catch(fail);
      if (note?.trim()) await post(`/api/dockets/${r.docket}/note`, { body: note }).catch(fail);
      toast(`${r.docket} created`);
      reload();
      return r.docket;
    },
  };
  return <HubCtx.Provider value={hub}>{children}</HubCtx.Provider>;
}

export const useHub = () => {
  const h = useContext(HubCtx);
  if (!h) throw new Error("useHub outside HubProvider");
  return h;
};

// ---- routes ----

export function useRoute(): string[] {
  const read = () => decodeURIComponent(window.location.hash.replace(/^#\/?/, "")).split("/").filter(Boolean);
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const on = () => setRoute(read());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}

export const go = (path: string) => {
  window.location.hash = path;
};

/** The list you came from, so a docket page can step through it (j/k) and go back (Esc). */
export const lastList: { path: string; title: string; ids: string[] } = { path: "/view/active", title: "Active", ids: [] };

// ---- time ----

export function ago(iso?: string | null): string {
  if (!iso) return "";
  const s = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  if (s < 86400 * 7) return `${Math.floor(s / 86400)}d`;
  return new Date(iso).toLocaleDateString([], { month: "short", day: "numeric" });
}

export function agoLong(iso?: string | null): string {
  const a = ago(iso);
  if (!a) return "";
  if (a === "now") return "just now";
  return /\d[mhd]$/.test(a) ? `${a} ago` : a;
}

export function stamp(iso: string): string {
  return new Date(iso).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false });
}

export const isToday = (iso?: string | null) => !!iso && new Date(iso).toDateString() === new Date().toDateString();

export function store<T>(key: string, fallback: T): [T, (v: T) => void] {
  let v = fallback;
  try {
    const raw = localStorage.getItem(`wh:${key}`);
    if (raw) v = JSON.parse(raw) as T;
  } catch {
    /* no storage */
  }
  return [
    v,
    (nv: T) => {
      try {
        localStorage.setItem(`wh:${key}`, JSON.stringify(nv));
      } catch {
        /* no storage */
      }
    },
  ];
}

// ---- the theme: System (follow macOS), Light, or Dark ----

export type Theme = "system" | "light" | "dark";
const themeListeners = new Set<(t: Theme) => void>();

export function currentTheme(): Theme {
  const [t] = store<Theme>("theme", "system");
  return t === "light" || t === "dark" ? t : "system";
}

/** System leaves the mode to the OS; Light and Dark pin it (see index.html). */
export function setTheme(t: Theme) {
  const root = document.documentElement;
  if (t === "system") delete root.dataset.mode;
  else root.dataset.mode = t;
  store<Theme>("theme", "system")[1](t);
  themeListeners.forEach((f) => f(t));
}

export function useTheme(): [Theme, (t: Theme) => void] {
  const [t, setT] = useState<Theme>(currentTheme);
  useEffect(() => {
    themeListeners.add(setT);
    return () => {
      themeListeners.delete(setT);
    };
  }, []);
  return [t, setTheme];
}
