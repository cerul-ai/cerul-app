import { Copy, Loader2, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import { writeClipboardText } from "../lib/clipboard";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import { EmptyState, InlineNotice } from "../components/leaf";

export function MomentsScreen({
  actionsEnabled,
  onOpenItem,
}: {
  actionsEnabled: boolean;
  onOpenItem: (moment: api.MomentRecord) => void;
}) {
  const t = useT();
  const [moments, setMoments] = useState<api.MomentRecord[]>([]);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [message, setMessage] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "error">("idle");

  // Same 1.6s reset as the detail view; the button used to stay "Copied"
  // forever, giving no feedback on subsequent copies.
  useEffect(() => {
    if (copyStatus === "idle") {
      return;
    }
    const timeout = window.setTimeout(() => setCopyStatus("idle"), 1600);
    return () => window.clearTimeout(timeout);
  }, [copyStatus]);

  const load = useCallback(async () => {
    if (!actionsEnabled) {
      setStatus("ready");
      setMoments([]);
      return;
    }
    setStatus("loading");
    setMessage(null);
    try {
      setMoments(await api.listMoments());
      setStatus("ready");
    } catch (error) {
      setMessage(errorMessage(error));
      setStatus("error");
    }
  }, [actionsEnabled]);

  useEffect(() => {
    void load();
  }, [load]);

  async function remove(moment: api.MomentRecord) {
    try {
      await api.deleteMoment(moment.id);
      await load();
    } catch (error) {
      setMessage(errorMessage(error));
    }
  }

  async function copyMarkdown() {
    const markdown = moments
      .map((moment) => `- [${moment.timestamp}] ${moment.quote}\n  - ${moment.title}`)
      .join("\n");
    try {
      await writeClipboardText(markdown);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("error");
    }
  }

  return (
    <div className="page wide">
      <div className="page-head row" style={{ alignItems: "flex-end", justifyContent: "space-between" }}>
        <div>
          <p className="page-eyebrow">{t("moments.eyebrow")}</p>
          <h1 className="page-h1">{t("moments.heading")}</h1>
          <p className="page-sub">{t("moments.sub")}</p>
        </div>
        <button
          type="button"
          className="btn btn-secondary sm"
          disabled={moments.length === 0}
          onClick={() => void copyMarkdown()}
        >
          <Copy size={15} />
          <span>{copyStatus === "copied" ? t("detail.copy.copied") : t("moments.copyMarkdown")}</span>
        </button>
      </div>
      {message ? <InlineNotice tone={status === "error" ? "error" : "muted"} message={message} /> : null}
      {copyStatus === "error" ? <InlineNotice tone="error" message={t("detail.copy.error")} /> : null}
      {status === "loading" ? (
        <div className="state"><Loader2 size={22} className="spin" /><span>{t("common.loading")}</span></div>
      ) : null}
      {status !== "loading" && moments.length === 0 ? (
        <EmptyState
          title={t("moments.empty.title")}
          body={t("moments.empty.body")}
        />
      ) : null}
      {moments.length > 0 ? (
        <div className="moments-list">
          {moments.map((moment) => (
            <article className="moment-card" key={moment.id}>
              <button type="button" className="moment-card__main" onClick={() => onOpenItem(moment)}>
                <span className="mono moment-card__time">{moment.timestamp}</span>
                <strong>{moment.title}</strong>
                <p>{moment.quote}</p>
              </button>
              <button
                type="button"
                className="btn-icon sm"
                aria-label={t("moments.unsave")}
                onClick={() => void remove(moment)}
              >
                <Trash2 size={15} />
              </button>
            </article>
          ))}
        </div>
      ) : null}
    </div>
  );
}
