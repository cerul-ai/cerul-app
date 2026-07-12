// P3 connector console — groups real source records by connector type, keeps
// actions available in the detail list, and turns sync health into a scanable
// workspace instead of leading with raw URLs.

import {
  Activity,
  AlertTriangle,
  ChevronRight,
  Clapperboard,
  FileVideo,
  Folder,
  Plus,
  Podcast,
  RefreshCcw,
  Youtube,
} from "lucide-react";
import { useMemo, useState } from "react";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { RequestConfirm, Source } from "../lib/types";
import { EmptyState, InlineNotice } from "../components/leaf";
import { SourceRow } from "../components/source-row";

type SourceView = "all" | "syncing" | "attention" | "history";
type ConnectorKind = "youtube" | "local" | "podcast" | "web";

function connectorKind(source: Source): ConnectorKind {
  if (source.type === "youtube") return "youtube";
  if (source.type === "podcast") return "podcast";
  if (source.type === "web_video") return "web";
  return "local";
}

function connectorDisplayName(source: Source, fallback: string): string {
  if (source.type === "folder" || source.type === "file") {
    const clean = source.name.replace(/[\\/]+$/, "");
    return clean.split(/[\\/]/).pop() || fallback;
  }
  try {
    const url = new URL(source.name.includes("://") ? source.name : `https://${source.name}`);
    const host = url.hostname.replace(/^www\./, "");
    const parts = url.pathname.split("/").filter(Boolean);
    if (host.includes("bilibili.com")) {
      const authorId = host === "space.bilibili.com" ? parts[0] : null;
      const videoId = parts.find((part) => /^BV/i.test(part));
      if (authorId) return `Bilibili · ${authorId}`;
      if (videoId) return `Bilibili · ${videoId}`;
      return "Bilibili 视频";
    }
    if (host.includes("youtube.com") || host === "youtu.be") {
      const videoId = url.searchParams.get("v") || parts.at(-1);
      return videoId ? `YouTube · ${videoId}` : "YouTube 视频";
    }
    return host;
  } catch {
    return source.name || fallback;
  }
}

export function SourcesScreen({
  sources,
  actionsEnabled,
  onAddSource,
  onPauseSource,
  onResumeSource,
  onRemoveSource,
  onRetryFailedSource,
  onRetrySourceDiscovery,
  onViewItems,
  onOpenSettingsFix,
  requestConfirm,
}: {
  sources: Source[];
  actionsEnabled: boolean;
  onAddSource: () => void;
  onPauseSource: (source: Source) => Promise<void>;
  onResumeSource: (source: Source) => Promise<void>;
  onRemoveSource: (source: Source) => Promise<void>;
  onRetryFailedSource: (source: Source) => Promise<void>;
  onRetrySourceDiscovery: (source: Source) => Promise<void>;
  onViewItems: (source: Source) => void;
  onOpenSettingsFix: (section: string) => void;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [view, setView] = useState<SourceView>("all");
  const [kind, setKind] = useState<ConnectorKind | null>(null);
  const [pendingSourceId, setPendingSourceId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  async function runSourceAction(source: Source, action: () => Promise<void>) {
    if (!actionsEnabled) {
      setActionError(t("sources.coreUnreachable"));
      return;
    }

    setPendingSourceId(source.id);
    setActionError(null);
    try {
      await action();
    } catch (error) {
      setActionError(errorMessage(error));
    } finally {
      setPendingSourceId(null);
    }
  }

  async function removeSource(source: Source) {
    const confirmed = await requestConfirm({
      title: t("sources.confirm.title"),
      body: t("sources.confirm.body", { name: source.name }),
      confirmLabel: t("sources.confirm.title"),
    });
    if (!confirmed) return;
    await runSourceAction(source, () => onRemoveSource(source));
  }

  async function retryFailedSource(source: Source) {
    const confirmed = await requestConfirm({
      title: t("sources.retryFailed.confirm.title"),
      body: t("sources.retryFailed.confirm.body", { count: source.failedItems }),
      confirmLabel: t("sources.retryFailed.confirm.confirm"),
    });
    if (!confirmed) return;
    await runSourceAction(source, () => onRetryFailedSource(source));
  }

  const totalItems = sources.reduce((sum, source) => sum + source.items, 0);
  const syncingCount = sources.filter((source) => source.status === "syncing").length;
  const attentionCount = sources.filter((source) => source.status === "error" || source.failedItems > 0).length;

  const viewSources = useMemo(() => sources.filter((source) => {
    if (view === "syncing" && source.status !== "syncing") return false;
    if (view === "attention" && source.status !== "error" && source.failedItems === 0) return false;
    if (kind && connectorKind(source) !== kind) return false;
    return true;
  }), [kind, sources, view]);

  const connectorGroups: Array<{
    id: ConnectorKind;
    title: string;
    short: string;
    Icon: typeof Youtube;
  }> = [
    { id: "youtube", title: t("sources.p3.youtube"), short: "YT", Icon: Youtube },
    { id: "local", title: t("sources.p3.local"), short: "LOCAL", Icon: Folder },
    { id: "podcast", title: t("sources.p3.podcast"), short: "RSS", Icon: Podcast },
    { id: "web", title: t("sources.p3.web"), short: "WEB", Icon: Clapperboard },
  ];

  const sourceViews: Array<{ id: SourceView; label: string; count?: number }> = [
    { id: "all", label: t("sources.p3.all"), count: sources.length },
    { id: "syncing", label: t("sources.p3.syncing"), count: syncingCount },
    { id: "attention", label: t("sources.p3.attention"), count: attentionCount },
    { id: "history", label: t("sources.p3.history") },
  ];

  function renderRow(source: Source) {
    const displaySource = {
      ...source,
      name: connectorDisplayName(source, t("sources.p3.unnamed")),
    };
    return (
      <SourceRow
        key={source.id}
        source={displaySource}
        actionsEnabled={actionsEnabled}
        isPending={pendingSourceId === source.id}
        onPause={() => void runSourceAction(source, () => onPauseSource(source))}
        onResume={() => void runSourceAction(source, () => onResumeSource(source))}
        onRemove={() => void removeSource(source)}
        onRetryFailed={() => void retryFailedSource(source)}
        onRetryDiscovery={() => void runSourceAction(source, () => onRetrySourceDiscovery(source))}
        onFix={() => {
          if (source.fixSettingsSection) onOpenSettingsFix(source.fixSettingsSection);
          else onAddSource();
        }}
        onViewItems={() => onViewItems(source)}
      />
    );
  }

  return (
    <div className="page wide p3-page sources-p3-page">
      <header className="p3-page-head">
        <div>
          <p className="page-eyebrow">P3 · {t("sources.p3.eyebrow")}</p>
          <h1 className="page-h1">{t("sources.p3.title")}</h1>
          <p className="page-sub">{t("sources.p3.sub", { count: sources.length, items: totalItems })}</p>
        </div>
        <button className="btn btn-primary" type="button" onClick={onAddSource}>
          <Plus size={16} />
          <span>{t("sources.addSource")}</span>
        </button>
      </header>

      {actionError ? <InlineNotice tone="error" message={actionError} /> : null}

      <div className="p3-workspace">
        <aside className="p3-side card" aria-label={t("sources.p3.viewsAria")}>
          <h2>{t("sources.p3.views")}</h2>
          <nav>
            {sourceViews.map((entry) => (
              <button
                type="button"
                key={entry.id}
                className={view === entry.id ? "active" : ""}
                aria-current={view === entry.id ? "page" : undefined}
                onClick={() => setView(entry.id)}
              >
                <span>{entry.label}</span>
                {entry.count !== undefined ? <code>{entry.count}</code> : null}
                {view === entry.id ? <ChevronRight size={15} /> : null}
              </button>
            ))}
          </nav>
          <button type="button" className="p3-side-add" onClick={onAddSource}>
            <Plus size={14} />
            {t("sources.addSource")}
          </button>
        </aside>

        <main className="p3-main-scroll">
          <section className="connector-grid" aria-label={t("sources.p3.connectorsAria")}>
            {connectorGroups.map(({ id, title, short, Icon }) => {
              const groupSources = sources.filter((source) => connectorKind(source) === id);
              const groupItems = groupSources.reduce((sum, source) => sum + source.items, 0);
              const hasError = groupSources.some((source) => source.status === "error" || source.failedItems > 0);
              const isSyncing = groupSources.some((source) => source.status === "syncing");
              return (
                <button
                  type="button"
                  className={kind === id ? "connector-card active" : "connector-card"}
                  key={id}
                  onClick={() => setKind((current) => current === id ? null : id)}
                >
                  <span className="connector-icon" aria-hidden="true"><Icon size={18} /><small>{short}</small></span>
                  <span className="connector-copy">
                    <strong>{title} · {groupSources.length}</strong>
                    <small>{t("sources.p3.connectorMeta", { items: groupItems })}</small>
                  </span>
                  <span className={hasError ? "connector-health error" : isSyncing ? "connector-health syncing" : "connector-health"}>
                    {hasError ? t("sources.p3.needsAction") : isSyncing ? t("sources.p3.syncingNow") : groupSources.length > 0 ? t("sources.p3.connected") : t("sources.p3.notConfigured")}
                  </span>
                </button>
              );
            })}
          </section>

          <section className="connector-activity card">
            <header>
              <span><Activity size={15} /><strong>{view === "history" ? t("sources.p3.history") : t("sources.p3.activity")}</strong></span>
              {kind ? <button type="button" onClick={() => setKind(null)}>{t("sources.p3.clearType")}</button> : null}
            </header>
            {viewSources.length > 0 ? (
              <div className="connector-timeline">
                {viewSources.slice(0, 8).map((source) => (
                  <button type="button" key={source.id} onClick={() => onViewItems(source)}>
                    <time>{source.lastPolled || "—"}</time>
                    <i data-tone={source.status} />
                    <span>
                      <strong>{connectorDisplayName(source, t("sources.p3.unnamed"))}</strong>
                      <small>{source.status === "error" ? source.error || t("sources.p3.needsAction") : t("sources.p3.activityMeta", { items: source.items })}</small>
                    </span>
                    {source.status === "error" ? <AlertTriangle size={14} /> : source.status === "syncing" ? <RefreshCcw size={14} className="spin" /> : <ChevronRight size={14} />}
                  </button>
                ))}
              </div>
            ) : (
              <EmptyState
                title={sources.length === 0 ? t("sources.empty.title") : t("sources.p3.noMatch")}
                body={sources.length === 0 ? t("sources.empty.body") : t("sources.p3.noMatchBody")}
                actionLabel={sources.length === 0 ? t("sources.addSource") : undefined}
                onAction={sources.length === 0 ? onAddSource : undefined}
              />
            )}
          </section>

          {view !== "history" && viewSources.length > 0 ? (
            <section className="connector-detail card">
              <header>
                <span><FileVideo size={15} /><strong>{t("sources.p3.details")}</strong></span>
                <small>{t("sources.p3.detailsCount", { count: viewSources.length })}</small>
              </header>
              <div className="source-list">{viewSources.map(renderRow)}</div>
            </section>
          ) : null}
        </main>
      </div>
    </div>
  );
}
