// C4 connector console — groups real source records by connector type, keeps
// actions available in the detail list, and turns sync health into a scanable
// workspace instead of leading with raw URLs.

import {
  Activity,
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Clapperboard,
  Folder,
  Globe2,
  Plus,
  Podcast,
  RefreshCcw,
  Youtube,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { RequestConfirm, Source } from "../lib/types";
import { EmptyState, InlineNotice } from "../components/leaf";
import { SourceRow } from "../components/source-row";

type SourceView = "all" | "syncing" | "attention" | "history";
type ConnectorKind = "bilibili" | "youtube" | "local" | "podcast" | "web";

function connectorKind(source: Source): ConnectorKind {
  if (source.type === "youtube") return "youtube";
  if (source.type === "podcast") return "podcast";
  if (source.type === "folder" || source.type === "file") return "local";
  try {
    const url = new URL(source.name.includes("://") ? source.name : `https://${source.name}`);
    const host = url.hostname.replace(/^www\./, "");
    if (isBilibiliHost(host)) return "bilibili";
    if (isHostOrSubdomain(host, "youtube.com") || host === "youtu.be") return "youtube";
  } catch {
    // Non-URL web sources stay in the generic web-video group.
  }
  return "web";
}

function isHostOrSubdomain(host: string, domain: string): boolean {
  return host === domain || host.endsWith(`.${domain}`);
}

function isBilibiliHost(host: string): boolean {
  return isHostOrSubdomain(host, "bilibili.com") || isHostOrSubdomain(host, "b23.tv");
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
    if (isBilibiliHost(host)) {
      const authorId = host === "space.bilibili.com" ? parts[0] : null;
      const videoId = parts.find((part) => /^BV/i.test(part));
      if (authorId) return `Bilibili · ${authorId}`;
      if (videoId) return `Bilibili · ${videoId}`;
      return "Bilibili 视频";
    }
    if (isHostOrSubdomain(host, "youtube.com") || host === "youtu.be") {
      const channelId = parts.find((part) => part.startsWith("@") || /^UC[\w-]+$/i.test(part));
      const videoId = url.searchParams.get("v") || (host === "youtu.be" ? parts[0] : null);
      const label = channelId || videoId;
      return label ? `YouTube · ${label}` : "YouTube 视频";
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
  const [expandedKinds, setExpandedKinds] = useState<Set<ConnectorKind>>(
    () => new Set(["bilibili"]),
  );
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
    return true;
  }), [sources, view]);

  const connectorGroups: Array<{
    id: ConnectorKind;
    title: string;
    short: string;
    Icon: typeof Youtube;
  }> = [
    { id: "bilibili", title: t("sources.p3.bilibili"), short: "BILI", Icon: Clapperboard },
    { id: "youtube", title: t("sources.p3.youtube"), short: "YT", Icon: Youtube },
    { id: "local", title: t("sources.p3.local"), short: "LOCAL", Icon: Folder },
    { id: "podcast", title: t("sources.p3.podcast"), short: "RSS", Icon: Podcast },
    { id: "web", title: t("sources.p3.web"), short: "WEB", Icon: Globe2 },
  ];

  const connectorGroupsWithSources = connectorGroups
    .map((group) => ({
      ...group,
      sources: viewSources
        .filter((source) => connectorKind(source) === group.id)
        .sort((a, b) => {
          const aAttention = a.status === "error" || a.failedItems > 0 ? 1 : 0;
          const bAttention = b.status === "error" || b.failedItems > 0 ? 1 : 0;
          return bAttention - aAttention;
        }),
    }))
    .filter((group) => sources.length > 0 && (view === "all" || group.sources.length > 0));

  const attentionKindSignature = useMemo(
    () => Array.from(new Set(
      sources
        .filter((source) => source.status === "error" || source.failedItems > 0)
        .map(connectorKind),
    )).sort().join("|"),
    [sources],
  );

  useEffect(() => {
    if (!attentionKindSignature) return;
    setExpandedKinds((current) => {
      const next = new Set(current);
      attentionKindSignature.split("|").forEach((kind) => next.add(kind as ConnectorKind));
      return next;
    });
  }, [attentionKindSignature]);

  function toggleConnectorGroup(kind: ConnectorKind) {
    setExpandedKinds((current) => {
      const next = new Set(current);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  }

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
          <p className="page-eyebrow">{t("sources.p3.eyebrow")}</p>
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
              </button>
            ))}
          </nav>
          <button type="button" className="p3-side-add" onClick={onAddSource}>
            <Plus size={14} />
            {t("sources.addSource")}
          </button>
        </aside>

        <main className="p3-main-scroll source-groups" aria-label={t("sources.p3.connectorsAria")}>
          {view === "history" ? (
            <section className="connector-activity card">
              <header>
                <span><Activity size={15} /><strong>{t("sources.p3.history")}</strong></span>
              </header>
              {sources.length > 0 ? (
                <div className="connector-timeline">
                  {sources.slice(0, 8).map((source) => (
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
                  title={t("sources.empty.title")}
                  body={t("sources.empty.body")}
                  actionLabel={t("sources.addSource")}
                  onAction={onAddSource}
                />
              )}
            </section>
          ) : connectorGroupsWithSources.length > 0 ? connectorGroupsWithSources.map(({ id, title, short, Icon, sources: groupSources }) => {
            const expanded = expandedKinds.has(id);
            const groupItems = groupSources.reduce((sum, source) => sum + source.items, 0);
            const hasError = groupSources.some((source) => source.status === "error" || source.failedItems > 0);
            const isSyncing = groupSources.some((source) => source.status === "syncing");
            return (
              <section className={expanded ? "source-group card expanded" : "source-group card"} key={id}>
                <button
                  type="button"
                  className="source-group-toggle"
                  aria-expanded={expanded}
                  onClick={() => toggleConnectorGroup(id)}
                >
                  <span className="source-group-icon" aria-hidden="true"><Icon size={17} /><small>{short}</small></span>
                  <span className="source-group-copy">
                    <strong>{title} · {t("sources.p3.detailsCount", { count: groupSources.length })}</strong>
                    <small>{groupSources.length > 0 ? t("sources.p3.connectorMeta", { items: groupItems }) : t("sources.p3.notConfigured")}</small>
                  </span>
                  {hasError ? <span className="source-group-state error"><AlertTriangle size={13} />{t("sources.p3.needsAction")}</span> : isSyncing ? <span className="source-group-state syncing">{t("sources.p3.syncingNow")}</span> : null}
                  <span className="source-group-action">{expanded ? t("sources.p3.collapse") : t("sources.p3.expand")}{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</span>
                </button>
                {expanded ? (
                  groupSources.length > 0 ? <div className="source-group-list">{groupSources.map(renderRow)}</div> : (
                    <div className="source-group-empty"><span>{t("sources.p3.notConfigured")}</span><button type="button" onClick={onAddSource}>{t("sources.addSource")}</button></div>
                  )
                ) : null}
              </section>
            );
          }) : (
            <EmptyState
              title={sources.length === 0 ? t("sources.empty.title") : t("sources.p3.noMatch")}
              body={sources.length === 0 ? t("sources.empty.body") : t("sources.p3.noMatchBody")}
              actionLabel={sources.length === 0 ? t("sources.addSource") : undefined}
              onAction={sources.length === 0 ? onAddSource : undefined}
            />
          )}
        </main>
      </div>
    </div>
  );
}
