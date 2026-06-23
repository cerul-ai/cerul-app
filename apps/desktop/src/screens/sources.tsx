// Sources screen — lists watched sources, surfaces inline errors, and
// gates destructive actions behind ConfirmDialog. Extracted from
// App.tsx (B13 Phase C).
//
// The screen owns the per-row "pending" state (so a row spinner appears
// during pause / resume / remove) but every cross-component action
// runs through host-supplied callbacks so the screen never touches
// the API directly.

import { Plus } from "lucide-react";
import { useState } from "react";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { RequestConfirm, Source } from "../lib/types";
import { EmptyState, InlineNotice } from "../components/leaf";
import { SourceRow } from "../components/source-row";

export function SourcesScreen({
  sources,
  actionsEnabled,
  onAddSource,
  onPauseSource,
  onResumeSource,
  onRemoveSource,
  onRetryFailedSource,
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
  onViewItems: (source: Source) => void;
  onOpenSettingsFix: (section: string) => void;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
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
    if (!confirmed) {
      return;
    }

    await runSourceAction(source, () => onRemoveSource(source));
  }

  async function retryFailedSource(source: Source) {
    const confirmed = await requestConfirm({
      title: t("sources.retryFailed.confirm.title"),
      body: t("sources.retryFailed.confirm.body", { count: source.failedItems }),
      confirmLabel: t("sources.retryFailed.confirm.confirm"),
    });
    if (!confirmed) {
      return;
    }

    await runSourceAction(source, () => onRetryFailedSource(source));
  }

  const groups = [
    { type: "folder" as const, label: t("sources.group.folder") },
    { type: "web_video" as const, label: t("sources.group.webVideo") },
    { type: "youtube" as const, label: t("sources.group.youtube") },
    { type: "podcast" as const, label: t("sources.group.podcast") },
    { type: "file" as const, label: t("sources.group.file") },
  ];
  const totalItems = sources.reduce((sum, source) => sum + source.items, 0);

  function renderRow(source: Source) {
    return (
      <SourceRow
        key={source.id}
        source={source}
        actionsEnabled={actionsEnabled}
        isPending={pendingSourceId === source.id}
        onPause={() => void runSourceAction(source, () => onPauseSource(source))}
        onResume={() => void runSourceAction(source, () => onResumeSource(source))}
        onRemove={() => void removeSource(source)}
        onRetryFailed={() => void retryFailedSource(source)}
        onFix={() => {
          if (source.fixSettingsSection) {
            onOpenSettingsFix(source.fixSettingsSection);
          } else {
            onAddSource();
          }
        }}
        onViewItems={() => onViewItems(source)}
      />
    );
  }

  return (
    <div className="page wide">
      <div className="page-head">
        <div className="page-eyebrow">{t("sources.eyebrow")}</div>
        <div className="row" style={{ justifyContent: "space-between", alignItems: "flex-end", gap: 16 }}>
          <div>
            <h1 className="page-h1">{t("sources.title")}</h1>
            {sources.length > 0 ? (
              <p className="page-sub">
                {t("sources.pageSub", { count: sources.length, items: totalItems })}
              </p>
            ) : null}
          </div>
          <button className="btn btn-primary" type="button" onClick={onAddSource}>
            <Plus size={16} />
            <span>{t("sources.addSource")}</span>
          </button>
        </div>
      </div>

      {actionError ? <InlineNotice tone="error" message={actionError} /> : null}

      {sources.length > 0 ? (
        groups.map((group) => {
          const groupSources = sources.filter((source) => source.type === group.type);
          if (groupSources.length === 0) {
            return null;
          }
          return (
            <div key={group.type} className="source-group">
              <div className="row gap-2" style={{ alignItems: "center" }}>
                <span className="section-label">{group.label}</span>
                <span className="chip neutral source-group-count">
                  <span className="dot" />
                  {groupSources.length}
                </span>
              </div>
              <div className="card" style={{ overflow: "visible" }}>
                <div className="source-list">{groupSources.map(renderRow)}</div>
              </div>
            </div>
          );
        })
      ) : (
        <div className="card pad">
          <EmptyState
            title={t("sources.empty.title")}
            body={t("sources.empty.body")}
            actionLabel={t("sources.addSource")}
            onAction={onAddSource}
          />
        </div>
      )}
    </div>
  );
}
