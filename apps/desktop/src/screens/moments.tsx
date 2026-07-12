import {
  ChevronRight,
  Copy,
  Film,
  Image as ImageIcon,
  Loader2,
  Plus,
  Quote,
  Tags,
  Trash2,
  Video,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "../lib/api";
import { writeClipboardText } from "../lib/clipboard";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { Item } from "../lib/types";
import { EmptyState, InlineNotice } from "../components/leaf";

type SavedView = "all" | "quotes" | "videos" | "review";

export function MomentsScreen({
  actionsEnabled,
  items,
  onOpenItem,
}: {
  actionsEnabled: boolean;
  items: Item[];
  onOpenItem: (moment: api.MomentRecord) => void;
}) {
  const t = useT();
  const [moments, setMoments] = useState<api.MomentRecord[]>([]);
  const [view, setView] = useState<SavedView>("all");
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [message, setMessage] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "error">("idle");

  useEffect(() => {
    if (copyStatus === "idle") return;
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

  const itemById = useMemo(() => new Map(items.map((item) => [item.id, item])), [items]);
  const savedVideoCount = useMemo(() => new Set(moments.map((moment) => moment.item_id)).size, [moments]);
  const visibleMoments = view === "videos" ? [] : moments;
  const views: Array<{ id: SavedView; label: string; count: number }> = [
    { id: "all", label: t("moments.p3.all"), count: moments.length },
    { id: "quotes", label: t("moments.p3.quotes"), count: moments.length },
    { id: "videos", label: t("moments.p3.videos"), count: 0 },
    { id: "review", label: t("moments.p3.review"), count: moments.length },
  ];
  const collections = [
    { id: "all" as const, eyebrow: "01", title: t("moments.p3.collection.all"), description: t("moments.p3.collection.allDesc", { count: moments.length }), count: moments.length, Icon: Quote },
    { id: "quotes" as const, eyebrow: "02", title: t("moments.p3.collection.clips"), description: t("moments.p3.collection.clipsDesc", { count: savedVideoCount }), count: savedVideoCount, Icon: Film },
    { id: "videos" as const, eyebrow: "03", title: t("moments.p3.collection.frames"), description: t("moments.p3.collection.framesDesc"), count: 0, Icon: ImageIcon },
    { id: "review" as const, eyebrow: "04", title: t("moments.p3.collection.manual"), description: t("moments.p3.collection.manualDesc"), count: moments.length, Icon: Tags },
  ];

  async function remove(moment: api.MomentRecord) {
    try {
      await api.deleteMoment(moment.id);
      await load();
    } catch (error) {
      setMessage(errorMessage(error));
    }
  }

  async function copyMarkdown() {
    const markdown = moments.map((moment) => `- [${moment.timestamp}] ${moment.quote}\n  - ${moment.title}`).join("\n");
    try {
      await writeClipboardText(markdown);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("error");
    }
  }

  return (
    <div className="page wide p3-page saved-p3-page">
      <header className="p3-page-head">
        <div>
          <p className="page-eyebrow">P3 · {t("moments.p3.eyebrow")}</p>
          <h1 className="page-h1">{t("moments.p3.title")}</h1>
          <p className="page-sub">{t("moments.p3.sub")}</p>
        </div>
        <button type="button" className="btn btn-secondary sm" disabled={moments.length === 0} onClick={() => void copyMarkdown()}>
          <Copy size={15} />
          <span>{copyStatus === "copied" ? t("detail.copy.copied") : t("moments.copyMarkdown")}</span>
        </button>
      </header>

      {message ? <InlineNotice tone={status === "error" ? "error" : "muted"} message={message} /> : null}
      {copyStatus === "error" ? <InlineNotice tone="error" message={t("detail.copy.error")} /> : null}

      <div className="p3-workspace">
        <aside className="p3-side card" aria-label={t("moments.p3.title")}>
          <h2>{t("moments.p3.title")}</h2>
          <nav>
            {views.map((entry) => (
              <button type="button" key={entry.id} className={view === entry.id ? "active" : ""} aria-current={view === entry.id ? "page" : undefined} onClick={() => setView(entry.id)}>
                <span>{entry.label}</span><code>{entry.count}</code>{view === entry.id ? <ChevronRight size={15} /> : null}
              </button>
            ))}
          </nav>
          <button type="button" className="p3-side-add" disabled title={t("moments.p3.groupsSoon")}>
            <Plus size={14} />{t("moments.p3.newGroup")}
          </button>
        </aside>

        <main className="p3-main-scroll">
          <section className="saved-collection-grid" aria-label={t("moments.p3.collectionsAria")}>
            {collections.map(({ id, eyebrow, title, description, count, Icon }) => (
              <button type="button" key={title} className={view === id ? "saved-collection-card active" : "saved-collection-card"} onClick={() => setView(id)}>
                <span className="saved-collection-icon"><Icon size={16} /></span>
                <span className="page-eyebrow">{eyebrow} · {t("moments.p3.title")}</span>
                <strong>{title}</strong><small>{description}</small>
                <span className="saved-collection-foot">{t("moments.p3.updated")}<b>{count} <ChevronRight size={13} /></b></span>
              </button>
            ))}
          </section>

          <section className="saved-p3-list" aria-live="polite">
            {status === "loading" ? (
              <div className="state"><Loader2 size={22} className="spin" /><span>{t("common.loading")}</span></div>
            ) : visibleMoments.length > 0 ? visibleMoments.map((moment) => {
              const item = itemById.get(moment.item_id);
              return (
                <article className="saved-p3-row" key={moment.id}>
                  <button type="button" className="saved-p3-open" onClick={() => onOpenItem(moment)}>
                    <span className={item?.thumbnailUrl ? "saved-p3-thumb has-image" : "saved-p3-thumb"}>
                      {item?.thumbnailUrl ? <img src={item.thumbnailUrl} alt="" /> : <Video size={18} />}<code>{moment.timestamp}</code>
                    </span>
                    <span className="saved-p3-copy"><strong className="clamp1">“{moment.quote}”</strong><small className="clamp1">{moment.title} · {moment.timestamp}</small></span>
                    <ChevronRight size={16} />
                  </button>
                  <button type="button" className="btn-icon sm saved-p3-remove" aria-label={t("moments.unsave")} onClick={() => void remove(moment)}><Trash2 size={14} /></button>
                </article>
              );
            }) : (
              <div className="saved-p3-empty card">
                <EmptyState title={view === "all" || view === "quotes" ? t("moments.empty.title") : t("moments.p3.filteredEmpty")} body={view === "all" || view === "quotes" ? t("moments.empty.body") : t("moments.p3.filteredEmptyBody")} />
              </div>
            )}
          </section>
        </main>
      </div>
    </div>
  );
}
