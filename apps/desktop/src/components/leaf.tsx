// Leaf-level shared components extracted from App.tsx (B13 Phase B).
// These have no coupling to App state — they take everything via props
// so they're safe to lift out without rewiring.

import { Inbox, Plus } from "lucide-react";
import type { LucideIcon } from "lucide-react";

export function InlineNotice({
  tone,
  message,
}: {
  tone: "error" | "muted";
  message: string;
}) {
  return (
    <div className={`inline-notice ${tone}`} role={tone === "error" ? "alert" : undefined}>
      {message}
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
