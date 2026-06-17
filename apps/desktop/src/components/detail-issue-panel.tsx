// Failed-item issue panel for Result detail / Item detail screens.
// Extracted from App.tsx (B13 Phase B).
//
// Pure props-driven: the host owns all state and decides which actions
// are enabled. Renders a banner with a primary action (locate / open
// original / re-index) plus a destructive "remove from library" button.

import { useState } from "react";
import { AlertTriangle, Check, Copy, ExternalLink, Folder, Loader2, RefreshCcw, Trash2 } from "lucide-react";
import { useT } from "../lib/i18n";
import type { DetailIssue } from "../lib/types";

export function DetailIssuePanel({
  issue,
  actionStatus,
  actionsEnabled,
  hasOriginalUrl,
  onLocate,
  onOpenOriginal,
  onReindex,
  onRemove,
}: {
  issue: DetailIssue;
  actionStatus: "idle" | "locating" | "reindexing" | "deleting" | "queued" | "error";
  actionsEnabled: boolean;
  hasOriginalUrl: boolean;
  onLocate: () => void;
  onOpenOriginal: () => void;
  onReindex: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const busy =
    actionStatus === "locating" ||
    actionStatus === "reindexing" ||
    actionStatus === "deleting";

  function copyRawError() {
    if (!issue.rawError) {
      return;
    }
    void navigator.clipboard?.writeText(issue.rawError).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  }
  const primaryLabel =
    issue.primaryAction === "locate"
      ? t("detail.issue.locate")
      : issue.primaryAction === "open-original"
        ? t("detail.issue.openOriginal")
        : issue.primaryAction === "reindex"
          ? t("detail.issue.reindex")
          : null;
  const primaryDisabled =
    busy ||
    (issue.primaryAction !== "open-original" && !actionsEnabled) ||
    (issue.primaryAction === "open-original" && !hasOriginalUrl);

  function runPrimaryAction() {
    if (issue.primaryAction === "locate") {
      onLocate();
      return;
    }
    if (issue.primaryAction === "open-original") {
      onOpenOriginal();
      return;
    }
    if (issue.primaryAction === "reindex") {
      onReindex();
    }
  }

  return (
    <div className="detail-issue" role="alert">
      <div className="detail-issue-head">
        <AlertTriangle size={20} />
        <strong>{issue.title}</strong>
      </div>
      <p className="detail-issue-message">{issue.message}</p>
      {issue.rawError ? (
        <details className="detail-issue-tech">
          <summary>
            <span className="detail-issue-tech-label">{t("detail.issue.tech")}</span>
            <button
              className="detail-issue-copy"
              type="button"
              onClick={(event) => {
                event.preventDefault();
                copyRawError();
              }}
            >
              {copied ? <Check size={13} /> : <Copy size={13} />}
              <span>{copied ? t("detail.issue.copied") : t("detail.issue.copy")}</span>
            </button>
          </summary>
          <pre className="detail-issue-raw mono">{issue.rawError}</pre>
        </details>
      ) : null}
      <div className="detail-issue-actions">
        {primaryLabel ? (
          <button
            className="btn btn-secondary sm"
            type="button"
            disabled={primaryDisabled}
            onClick={runPrimaryAction}
          >
            {actionStatus === "locating" || actionStatus === "reindexing" ? (
              <Loader2 size={16} className="spin" />
            ) : issue.primaryAction === "locate" ? (
              <Folder size={16} />
            ) : issue.primaryAction === "open-original" ? (
              <ExternalLink size={16} />
            ) : (
              <RefreshCcw size={16} />
            )}
            <span>
              {actionStatus === "locating"
                ? t("detail.issue.locating")
                : actionStatus === "reindexing"
                  ? t("detail.issue.reindexing")
                  : primaryLabel}
            </span>
          </button>
        ) : null}
        <button
          className="btn btn-danger sm"
          type="button"
          disabled={busy || !actionsEnabled}
          onClick={onRemove}
        >
          {actionStatus === "deleting" ? <Loader2 size={16} className="spin" /> : <Trash2 size={16} />}
          <span>{actionStatus === "deleting" ? t("detail.issue.removing") : issue.removeLabel}</span>
        </button>
      </div>
    </div>
  );
}
