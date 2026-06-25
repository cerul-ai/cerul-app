// Source list row used by Sources screen. Extracted from App.tsx (B13
// Phase B). Owns its own local UI state (kebab menu open / error
// panel expanded); takes every cross-component action as a prop so
// the host decides what pause/resume/remove/fix actually do.

import {
  AlertTriangle,
  ChevronRight,
  Clapperboard,
  FileVideo,
  Folder,
  Library,
  Loader2,
  MoreHorizontal,
  Pause,
  Play,
  Podcast,
  RefreshCcw,
  Trash2,
  Wrench,
  Youtube,
} from "lucide-react";
import { useRef, useState } from "react";
import { useT } from "../lib/i18n";
import type { Source } from "../lib/types";
import { useClickOutside, useEscapeToClose } from "../lib/use-dismissable";
import { StatusBadge } from "./transcript";

export function SourceRow({
  source,
  actionsEnabled,
  isPending,
  onPause,
  onResume,
  onRemove,
  onRetryFailed,
  onRetryDiscovery,
  onFix,
  onViewItems,
}: {
  source: Source;
  actionsEnabled: boolean;
  isPending: boolean;
  onPause: () => void;
  onResume: () => void;
  onRemove: () => void;
  onRetryFailed: () => void;
  onRetryDiscovery: () => void;
  onFix: () => void;
  onViewItems: () => void;
}) {
  const t = useT();
  const [menuOpen, setMenuOpen] = useState(false);
  const [errorExpanded, setErrorExpanded] = useState(source.status === "error");
  const actionsRef = useRef<HTMLDivElement | null>(null);
  useEscapeToClose(() => setMenuOpen(false), menuOpen);
  useClickOutside(actionsRef, () => setMenuOpen(false), menuOpen);
  const Icon =
    source.type === "file"
      ? FileVideo
      : source.type === "folder"
        ? Folder
        : source.type === "youtube"
          ? Youtube
          : source.type === "web_video"
            ? Clapperboard
            : Podcast;
  const canRunAction = actionsEnabled && !isPending;
  const canRetryDiscovery =
    (source.status === "error" || source.status === "syncing") &&
    (source.type === "youtube" || source.type === "web_video" || source.type === "podcast");
  const toggleLabel = source.status === "paused" ? t("sourceRow.resume") : t("sourceRow.pause");
  const statusLabel =
    source.status === "active"
      ? t("sourceRow.status.active")
      : source.status === "syncing"
        ? t("sourceRow.status.syncing")
      : source.status === "paused"
        ? t("sourceRow.status.paused")
        : t("sourceRow.status.error");

  function runAndClose(action: () => void) {
    setMenuOpen(false);
    action();
  }

  return (
    <article
      className={source.status === "error" ? "tbl-row source-row source-row-error" : "tbl-row source-row"}
    >
      <span className="source-icon thumb">
        <Icon size={18} />
      </span>
      <div>
        <strong className="clamp1">{source.name}</strong>
        <span className="muted">
          {t(source.items === 1 ? "sourceRow.itemCountOne" : "sourceRow.itemCountOther", { count: source.items })}
          {source.failedItems > 0
            ? ` · ${t(source.failedItems === 1 ? "sourceRow.failedCountOne" : "sourceRow.failedCountOther", { count: source.failedItems })}`
            : ""}
          {" · "}
          {t("sourceRow.lastPolled", { when: source.lastPolled })}
        </span>
      </div>
      {source.status === "error" ? (
        <button
          className="source-error-toggle"
          type="button"
          aria-expanded={errorExpanded}
          onClick={() => setErrorExpanded((expanded) => !expanded)}
        >
          <StatusBadge status={source.status} label={statusLabel} />
          <ChevronRight size={14} />
        </button>
      ) : (
        <StatusBadge status={source.status} label={statusLabel} />
      )}
      <div className="row-actions" ref={actionsRef}>
        <button
          className="btn-icon"
          type="button"
          aria-label={t("sourceRow.moreActionsAria")}
          aria-expanded={menuOpen}
          disabled={isPending}
          onClick={() => setMenuOpen((open) => !open)}
        >
          {isPending ? <Loader2 size={16} /> : <MoreHorizontal size={16} />}
        </button>
        {menuOpen ? (
          <div className="menu source-action-menu">
            <button
              type="button"
              disabled={!canRunAction}
              onClick={() => runAndClose(source.status === "paused" ? onResume : onPause)}
            >
              {source.status === "paused" ? <Play size={15} /> : <Pause size={15} />}
              <span>{toggleLabel}</span>
            </button>
            <button type="button" onClick={() => runAndClose(onViewItems)}>
              <Library size={15} />
              <span>{t("sourceRow.viewItems")}</span>
            </button>
            {source.failedItems > 0 && (source.type === "youtube" || source.type === "web_video") ? (
              <button
                type="button"
                disabled={!canRunAction}
                onClick={() => runAndClose(onRetryFailed)}
              >
                <RefreshCcw size={15} />
                <span>{t("sourceRow.retryFailed")}</span>
              </button>
            ) : null}
            {canRetryDiscovery ? (
              <button
                type="button"
                disabled={!canRunAction}
                onClick={() => runAndClose(onRetryDiscovery)}
              >
                <RefreshCcw size={15} />
                <span>{t("sourceRow.retryDiscovery")}</span>
              </button>
            ) : null}
            <button
              className="danger"
              type="button"
              disabled={!canRunAction}
              onClick={() => runAndClose(onRemove)}
            >
              <Trash2 size={15} />
              <span>{t("sourceRow.removeSource")}</span>
            </button>
          </div>
        ) : null}
      </div>
      {source.status === "error" && errorExpanded ? (
        <div className="card pad source-error-panel">
          <AlertTriangle size={16} />
          <div>
            <strong>{t("sourceRow.errorTitle")}</strong>
            <span>{source.error ?? t("sourceRow.errorFallback")}</span>
          </div>
          <button type="button" className="btn btn-secondary sm source-error-fix" onClick={onFix}>
            <Wrench size={15} />
            <span>{t("sourceRow.fix")}</span>
          </button>
          {canRetryDiscovery ? (
            <button
              type="button"
              className="btn btn-secondary sm source-error-retry"
              disabled={!canRunAction}
              onClick={onRetryDiscovery}
            >
              <RefreshCcw size={15} />
              <span>{t("sourceRow.retryDiscovery")}</span>
            </button>
          ) : null}
          <button
            type="button"
            className="btn btn-danger sm source-error-remove"
            disabled={!canRunAction}
            onClick={onRemove}
          >
            <Trash2 size={15} />
            <span>{t("sourceRow.remove")}</span>
          </button>
        </div>
      ) : null}
    </article>
  );
}
