// Leaf-level shared components extracted from App.tsx (B13 Phase B).
// These have no coupling to App state — they take everything via props
// so they're safe to lift out without rewiring.

import { AlertTriangle, Inbox, Plus } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState } from "react";

export function InlineNotice({
  tone,
  message,
  action,
  detail,
  detailLabel,
}: {
  tone: "error" | "muted";
  message: string;
  /** Optional recovery action (e.g. Retry) rendered at the trailing edge. */
  action?: { label: string; onClick: () => void };
  /** Raw technical detail (e.g. the underlying exception) shown behind a toggle
      so the human-readable `message` stays the headline. */
  detail?: string;
  detailLabel?: string;
}) {
  const [showDetail, setShowDetail] = useState(false);
  const hasDetail = Boolean(detail && detail !== message);
  return (
    <div className={`inline-notice ${tone}`} role={tone === "error" ? "alert" : undefined}>
      {tone === "error" ? <AlertTriangle size={15} className="inline-notice-icon" /> : null}
      <div className="inline-notice-body">
        <span>{message}</span>
        {hasDetail && showDetail ? <pre className="inline-notice-detail">{detail}</pre> : null}
      </div>
      {hasDetail ? (
        <button
          type="button"
          className="inline-notice-link"
          aria-expanded={showDetail}
          onClick={() => setShowDetail((open) => !open)}
        >
          {detailLabel ?? "Details"}
        </button>
      ) : null}
      {action ? (
        <button type="button" className="inline-notice-action" onClick={action.onClick}>
          {action.label}
        </button>
      ) : null}
    </div>
  );
}

export function EmptyState({
  title,
  body,
  actionLabel,
  onAction,
}: {
  title: string;
  body: string;
  actionLabel?: string;
  onAction?: () => void;
}) {
  return (
    <article className="state">
      <div className="state-icon">
        <Inbox size={20} />
      </div>
      <div className="state-title">{title}</div>
      <div className="state-sub">{body}</div>
      {actionLabel && onAction ? (
        <button className="btn btn-secondary sm" type="button" onClick={onAction}>
          <Plus size={16} />
          <span>{actionLabel}</span>
        </button>
      ) : null}
    </article>
  );
}

export function Metric({
  icon: Icon,
  label,
  value,
  actionLabel,
  onAction,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  actionLabel?: string;
  onAction?: () => void;
}) {
  return (
    <article className="card pad metric">
      <div className="row gap-2 metric-top">
        <Icon size={18} className="muted" />
        {actionLabel && onAction ? (
          <button type="button" className="btn btn-ghost sm metric-action" onClick={onAction}>
            {actionLabel}
          </button>
        ) : null}
      </div>
      <span className="section-label">{label}</span>
      <strong className="metric-value" title={value}>{value}</strong>
    </article>
  );
}
