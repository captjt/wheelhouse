// Images shared to Wheelhouse from outside it: the phone's share sheet
// (Android and Chrome, through the service worker: `#/share`), or an iOS
// Shortcut that stages them and opens `#/share/<digest>,<digest>`. Pick a
// docket, and they land in its composer, to caption, mark up and send.
import { useEffect, useMemo, useState } from "react";
import { post, type Docket } from "./api";
import { IconImage, StatusIcon } from "./icons";
import { go, useHub } from "./live";
import { handOn, imageUrl } from "./images";
import { useToast } from "./ui";

const SHARE_CACHE = "wh-share";

/** What the service worker kept from a share: the images, and any words. */
async function fromServiceWorker(): Promise<{ files: File[]; text: string }> {
  if (!("caches" in window)) return { files: [], text: "" };
  const c = await caches.open(SHARE_CACHE);
  const files: File[] = [];
  let text = "";
  for (const req of await c.keys()) {
    const res = await c.match(req);
    if (!res) continue;
    const path = new URL(req.url).pathname;
    if (path === "/share-inbox/text") text = await res.text();
    else if (path.startsWith("/share-inbox/file/")) {
      const name = decodeURIComponent(res.headers.get("x-name") || "shared.png");
      files.push(new File([await res.blob()], name, { type: res.headers.get("content-type") || "" }));
    }
    await c.delete(req);
  }
  return { files, text };
}

/** Images a Shortcut staged already, by digest: fetched back as files. */
async function fromDigests(list: string[]): Promise<File[]> {
  const out: File[] = [];
  for (const d of list) {
    if (!/^sha256:[0-9a-f]{64}$/i.test(d)) continue;
    const r = await fetch(imageUrl(d, "shared.png"));
    if (!r.ok) continue;
    const b = await r.blob();
    out.push(new File([b], `shared-${d.slice(7, 15)}${b.type === "image/jpeg" ? ".jpg" : ".png"}`, { type: b.type }));
  }
  return out;
}

export function ShareView({ staged }: { staged?: string }) {
  const hub = useHub();
  const toast = useToast();
  const [got, setGot] = useState<{ files: File[]; text: string } | null>(null);
  const [q, setQ] = useState("");
  const [title, setTitle] = useState("");
  const urls = useMemo(() => (got?.files ?? []).map((f) => URL.createObjectURL(f)), [got]);
  useEffect(() => () => urls.forEach((u) => URL.revokeObjectURL(u)), [urls]);

  useEffect(() => {
    let live = true;
    (staged ? fromDigests(staged.split(",")).then((files) => ({ files, text: "" })) : fromServiceWorker()).then(
      (g) => live && setGot(g),
      () => live && setGot({ files: [], text: "" }),
    );
    return () => {
      live = false;
    };
  }, [staged]);

  const open = (hub.state?.dockets ?? []).filter((d) => d.status === "todo" || d.status === "in_progress");
  const shown = open
    .filter((d) => !q.trim() || `${d.id} ${d.title}`.toLowerCase().includes(q.trim().toLowerCase()))
    .sort((a, b) => (b.updated_at || "").localeCompare(a.updated_at || ""))
    .slice(0, 30);

  const pick = (d: Pick<Docket, "id">) => {
    if (!got?.files.length) return;
    handOn(d.id, got.files, got.text);
    go(`/d/${d.id}`);
  };
  const create = () => {
    const t = title.trim();
    if (!t) return;
    post<{ docket: string }>("/api/dockets", { title: t }).then(
      (r) => pick({ id: r.docket }),
      (e: Error) => toast(e.message, "error"),
    );
  };

  return (
    <div className="view share-view">
      <header className="view-head">
        <span className="view-title">
          <IconImage size={15} /> Share to a docket
        </span>
      </header>
      <div className="share-body">
        {!got ? (
          <p className="muted">Getting the images…</p>
        ) : !got.files.length ? (
          <p className="muted">Nothing was shared, or it wasn't an image. Share a screenshot or a photo to Wheelhouse, or paste one into a docket's composer.</p>
        ) : (
          <>
            <div className="thumbs share-thumbs">
              {urls.map((u, i) => (
                <span key={u} className="thumb">
                  <img src={u} alt={got.files[i].name} />
                </span>
              ))}
            </div>
            <p className="muted small">
              {got.files.length === 1 ? "This image goes" : `These ${got.files.length} images go`} to the composer of the docket you pick, to caption, mark up and send to its agents.
            </p>
            <input className="share-search" placeholder="Find a docket…" value={q} autoFocus onChange={(e) => setQ(e.target.value)} />
            <div className="share-list">
              {shown.map((d) => (
                <button key={d.id} className="share-row" onClick={() => pick(d)}>
                  <StatusIcon status={d.status} />
                  <span className="row-id">{d.id}</span>
                  <span className="share-row-title">{d.title}</span>
                </button>
              ))}
              {!shown.length && <p className="muted small">No open docket matches.</p>}
            </div>
            <div className="share-new">
              <input placeholder="Or a new docket: its title" value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && create()} />
              <button className="btn btn-small" disabled={!title.trim()} onClick={create}>
                New docket
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
