import { useState } from "react";
import { get, post, type ReviewReady } from "./api";
import { IconExternal } from "./icons";
import { useHub } from "./live";
import { Modal, Prose, useToast } from "./ui";

/** Post, Preview and Dismiss for a review rex-reviewer has ready. Nothing is
 * sent unless it's still the review shown; rex-reviewer checks the round too. */
export function ReviewButtons({ r, small }: { r: ReviewReady; small?: boolean }) {
  const hub = useHub();
  const toast = useToast();
  const [busy, setBusy] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const cls = small ? "btn btn-small" : "btn";
  const act = (what: "post" | "dismiss") => (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (busy) return;
    setBusy(what);
    post<{ url?: string }>(`/api/reviews/${what}`, { item: r.item, msg: r.msg ?? null, updated_at: r.updated_at ?? null }).then(
      (res) => {
        toast(what === "post" ? `Posted the review of ${r.key}${res.url ? "" : ""}` : `Dismissed the review of ${r.key}`);
        setBusy(null);
        hub.reload();
      },
      (err: Error) => (setBusy(null), toast(err.message, "error"), hub.reload()),
    );
  };
  const show = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setBusy("preview");
    get<{ preview: string }>(`/api/reviews/preview?item=${encodeURIComponent(r.item)}`).then(
      (v) => (setBusy(null), setPreview(v.preview)),
      (err: Error) => (setBusy(null), toast(err.message, "error")),
    );
  };
  return (
    <span className="btn-pair answer">
      <button className={`${cls} btn-approve`} disabled={!!busy} onClick={act("post")} title={`Post: ${r.msg ?? ""}`}>
        {busy === "post" ? "Posting…" : "Post"}
      </button>
      <button className={cls} disabled={!!busy} onClick={show}>
        {busy === "preview" ? "Loading…" : "Preview"}
      </button>
      <button className={cls} disabled={!!busy} onClick={act("dismiss")}>
        {busy === "dismiss" ? "Dismissing…" : "Dismiss"}
      </button>
      {preview !== null && (
        <Modal onClose={() => setPreview(null)} className="preview-modal">
          <div className="nd-head">
            <span>
              Review of {r.key}
              {r.round ? ` · round ${r.round}` : ""}, as it would post
            </span>
            <span className="spacer" />
            {r.url && (
              <a className="btn btn-small btn-ghost" href={r.url} target="_blank" rel="noreferrer">
                PR <IconExternal size={11} />
              </a>
            )}
          </div>
          <div className="preview-body">
            <Prose text={preview} />
          </div>
          <div className="nd-foot">
            <span className="muted small">{r.msg}</span>
            <span className="spacer" />
            <button className="btn btn-small" onClick={act("dismiss")} disabled={!!busy}>
              Dismiss
            </button>
            <button className="btn btn-primary btn-small" onClick={(e) => (act("post")(e), setPreview(null))} disabled={!!busy}>
              Post review
            </button>
          </div>
        </Modal>
      )}
    </span>
  );
}
