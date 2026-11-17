import type { Docket } from "./api";

/** A docket's title. One named after its pane reads "Claude Code in rex-hub",
 * with the agent part quiet, so the place stands out. */
export function TitleText({ d }: { d: Pick<Docket, "title" | "title_source"> }) {
  const m = d.title_source === "pane" ? d.title.match(/^(.+?) in (.+)$/) : null;
  if (!m) return <>{d.title}</>;
  return (
    <>
      <span className="title-pre">{m[1]} in </span>
      {m[2]}
    </>
  );
}

/** The last two parts of a path: `~/git/captjt/rex-hub` → `captjt/rex-hub`. */
export function shortPath(p?: string | null): string {
  if (!p) return "";
  const parts = p.replace(/\/$/, "").split("/");
  return parts.length <= 3 ? p : parts.slice(-2).join("/");
}

/** A steady color for a Rex session, the way a team gets one. */
export function sessionHue(name: string): number {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (h * 7 + 200) % 360;
}

export function SessionIcon({ name, size = 16 }: { name: string; size?: number }) {
  return (
    <span className="session-icon" style={{ ["--h" as string]: sessionHue(name), width: size, height: size, fontSize: size * 0.62 }}>
      {(name.replace(/^rex-?/, "")[0] || name[0] || "?").toUpperCase()}
    </span>
  );
}

export function SessionChip({ name }: { name: string }) {
  return (
    <span className="chip" title={`Rex session ${name}`}>
      <span className="chip-dot" style={{ ["--h" as string]: sessionHue(name) }} />
      {name}
    </span>
  );
}

// Status: the docket's own state, drawn the way issue trackers do: an empty
// ring to do, a half-filled ring in progress, a filled check when done.
export function StatusIcon({ status, size = 14 }: { status: Docket["status"] | string; size?: number }) {
  const s = size;
  switch (status) {
    case "in_progress":
      return (
        <svg width={s} height={s} viewBox="0 0 14 14" className="si si-progress" aria-label="In progress">
          <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.5" />
          <path d="M7 3.5 A3.5 3.5 0 0 1 7 10.5 Z" fill="currentColor" />
        </svg>
      );
    case "done":
      return (
        <svg width={s} height={s} viewBox="0 0 14 14" className="si si-done" aria-label="Done">
          <circle cx="7" cy="7" r="6.5" fill="currentColor" />
          <path d="M4.3 7.2 6.2 9 9.8 5.2" fill="none" stroke="var(--main)" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case "dropped":
      return (
        <svg width={s} height={s} viewBox="0 0 14 14" className="si si-dropped" aria-label="Dropped">
          <circle cx="7" cy="7" r="6.5" fill="currentColor" />
          <path d="M5 5l4 4M9 5l-4 4" stroke="var(--main)" strokeWidth="1.5" strokeLinecap="round" />
        </svg>
      );
    default:
      return (
        <svg width={s} height={s} viewBox="0 0 14 14" className="si si-todo" aria-label="To do">
          <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      );
  }
}

// Live: what the agent on the docket is doing right now, from Rex.
export function LiveIcon({ state, size = 14 }: { state?: string | null; size?: number }) {
  const s = size;
  if (!state) return <span style={{ width: s, height: s, display: "inline-block" }} />;
  if (state === "blocked")
    return (
      <svg width={s} height={s} viewBox="0 0 14 14" className="li li-blocked" aria-label="Needs you">
        <rect x="1" y="1" width="12" height="12" rx="3.5" fill="currentColor" />
        <path d="M7 4v3.6" stroke="var(--main)" strokeWidth="1.6" strokeLinecap="round" />
        <circle cx="7" cy="10" r="0.95" fill="var(--main)" />
      </svg>
    );
  if (state === "error")
    return (
      <svg width={s} height={s} viewBox="0 0 14 14" className="li li-error" aria-label="Failed">
        <rect x="1" y="1" width="12" height="12" rx="3.5" fill="currentColor" />
        <path d="M5 5l4 4M9 5l-4 4" stroke="var(--main)" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    );
  if (state === "working")
    return (
      <span className="li li-working" style={{ width: s, height: s }} aria-label="Working">
        <span className="pulse" />
      </span>
    );
  // done (finished its turn), idle, starting
  return (
    <svg width={s} height={s} viewBox="0 0 14 14" className={`li li-${state}`} aria-label={state}>
      <circle cx="7" cy="7" r="3" fill="currentColor" />
    </svg>
  );
}

export const liveWord = (state?: string | null) =>
  ({ working: "Working", blocked: "Needs you", error: "Failed", done: "Finished", idle: "Idle", starting: "Starting" })[state ?? ""] ?? "";

// An agent's avatar: a rounded square with the app's mark.
export function AppAvatar({ app, size = 18 }: { app?: string | null; size?: number }) {
  const letter = app === "codex" ? "X" : app === "claude-code" ? "C" : "A";
  return (
    <span className={`avatar avatar-app avatar-${app || "agent"}`} style={{ width: size, height: size, fontSize: size * 0.55 }} title={app || "agent"}>
      {letter}
    </span>
  );
}

export function PersonAvatar({ name, size = 18 }: { name: string; size?: number }) {
  return (
    <span className="avatar avatar-person" style={{ width: size, height: size, fontSize: size * 0.55 }} title={name}>
      {(name[0] || "?").toUpperCase()}
    </span>
  );
}

export function HubAvatar({ size = 18 }: { size?: number }) {
  return (
    <span className="avatar avatar-hub" style={{ width: size, height: size }} title="Wheelhouse">
      <Logo size={size * 0.62} />
    </span>
  );
}

export function Logo({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden className="logo">
      <circle cx="16" cy="16" r="11" fill="none" stroke="currentColor" strokeWidth="3.2" />
      <circle cx="16" cy="16" r="3.4" fill="currentColor" />
      {[0, 60, 120, 180, 240, 300].map((a) => (
        <line key={a} x1="16" y1="2" x2="16" y2="5.5" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" transform={`rotate(${a} 16 16)`} />
      ))}
    </svg>
  );
}

type G = { size?: number };
const glyph = (d: string, fill = false) =>
  function Glyph({ size = 16 }: G) {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden className="glyph">
        <path d={d} fill={fill ? "currentColor" : "none"} stroke={fill ? "none" : "currentColor"} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  };

export const IconInbox = glyph("M2.5 9.5 4 3.5h8l1.5 6v3h-11v-3ZM2.5 9.5H6l.8 1.5h2.4L10 9.5h3.5");
export const IconActive = glyph("M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11ZM8 5v3l2 1.5");
export const IconLayers = glyph("M8 2.5 2.5 5.3 8 8.1l5.5-2.8L8 2.5ZM2.5 8 8 10.8 13.5 8M2.5 10.7 8 13.5l5.5-2.8");
export const IconDone = glyph("M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11ZM5.6 8.1l1.6 1.6 3.2-3.4");
export const IconAgents = glyph("M4 4.5h8a1.5 1.5 0 0 1 1.5 1.5v5A1.5 1.5 0 0 1 12 12.5H4A1.5 1.5 0 0 1 2.5 11V6A1.5 1.5 0 0 1 4 4.5ZM5.5 7.5l1.5 1.2-1.5 1.3M8.5 10h2");
export const IconSearch = glyph("M7 2.5a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9ZM10.3 10.3l3.2 3.2");
export const IconPlus = glyph("M8 3v10M3 8h10");
export const IconEdit = glyph("M3 13h3l7-7-3-3-7 7v3ZM9 4l3 3");
export const IconTerminal = glyph("M2.5 3.5h11v9h-11v-9ZM5 6.5l2 1.5-2 1.5M8.5 10h2.5");
export const IconList = glyph("M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01");
export const IconBoard = glyph("M2.5 3h3v10h-3V3ZM6.5 3h3v7h-3V3ZM10.5 3h3v5h-3V3Z");
export const IconChevron = glyph("M6 4l4 4-4 4");
export const IconChevronDown = glyph("M4 6l4 4 4-4");
export const IconUp = glyph("M4 10l4-4 4 4");
export const IconDown = glyph("M4 6l4 4 4-4");
export const IconCheck = glyph("M3.5 8.5l3 3 6-7");
export const IconShield = glyph("M8 2 3 4v4c0 3 2.2 5 5 6 2.8-1 5-3 5-6V4L8 2ZM5.8 8l1.6 1.6L10.4 6.5");
export const IconDownload = glyph("M8 2.5v8M5 7.5l3 3 3-3M3 13h10");
export const IconCopy = glyph("M5.5 5.5h7v7h-7v-7ZM3.5 10.5v-7h7");
export const IconMenu = glyph("M2.5 4.5h11M2.5 8h11M2.5 11.5h11");
export const IconDisplay = glyph("M2.5 4.5h7M11.5 4.5h2M2.5 11.5h2M6.5 11.5h7M9.5 3v3M4.5 10v3");
export const IconSession = glyph("M2.5 3.5h11v9h-11v-9ZM2.5 6h11");
export const IconEye = glyph("M1.8 8S4 3.8 8 3.8 14.2 8 14.2 8 12 12.2 8 12.2 1.8 8 1.8 8ZM8 6.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6Z");
export const IconX = glyph("M4 4l8 8M12 4l-8 8");
export const IconIssue = glyph("M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11ZM5.2 10.8l5.6-5.6M3.6 8.6l4.8-4.8M7.6 12.4l4.8-4.8");
export const IconHistory = glyph("M2.8 8a5.2 5.2 0 1 0 1.6-3.8M2.5 2.8v2.6h2.6M8 5.2V8l2 1.4");
export const IconExternal = glyph("M9 3h4v4M13 3 7.5 8.5M11.5 9.5V13H3V4.5h3.5");
export const IconBell = glyph("M4 11V7a4 4 0 0 1 8 0v4l1.2 1.5H2.8L4 11ZM6.6 13.8a1.5 1.5 0 0 0 2.8 0");
export const IconMonitor = glyph("M2.5 3.5h11v7.5h-11v-7.5ZM6 13.5h4M8 11v2.5");
export const IconSun = glyph("M8 5.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 0 0 0-5.6ZM8 1.8v1.4M8 12.8v1.4M1.8 8h1.4M12.8 8h1.4M3.6 3.6l1 1M11.4 11.4l1 1M3.6 12.4l1-1M11.4 4.6l1-1");
export const IconMoon = glyph("M12.8 9.6A5 5 0 0 1 6.4 3.2a5 5 0 1 0 6.4 6.4Z");
export const IconHook = glyph("M10.5 3v5.5a3 3 0 0 1-6 0V7M10.5 3 9 4.5M10.5 3 12 4.5");
