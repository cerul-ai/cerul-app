// Preview tile used by onboarding YouTube picker and Add Source RSS tab.
// Extracted from App.tsx (B13 Phase B).

import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";
import { useT } from "../lib/i18n";
import type { ValidationState } from "../lib/types";

export function SourcePreview({
  icon,
  initials,
  title,
  validation,
  idleMessage,
  validDetail,
  imageUrl,
}: {
  icon: ReactNode;
  initials: string;
  title: string;
  validation: ValidationState;
  idleMessage: string;
  validDetail: string;
  imageUrl?: string | null;
}) {
  const t = useT();

  if (validation.status === "error") {
    return (
      <div className="preview-row error">
        {icon}
        <span>{validation.message}</span>
      </div>
    );
  }

  if (validation.status === "validating") {
    return (
      <div className="preview-row muted">
        <Loader2 size={18} className="spin" />
        <span>{t("sourcePreview.checking")}</span>
      </div>
    );
  }

  if (validation.status === "valid") {
    return (
      <div className="preview-row">
        {imageUrl ? (
          <img
            className="preview-image thumb"
            src={imageUrl}
            alt=""
            referrerPolicy="no-referrer"
          />
        ) : (
          <span className="avatar thumb stripes">{initials}</span>
        )}
        <div>
          <strong>{title}</strong>
          {/* Body copy, not code: the monospace dash-joined line read like
              debug output. */}
          <span className="muted">{validation.message}</span>
          {validDetail ? <span className="muted">{validDetail}</span> : null}
        </div>
      </div>
    );
  }

  return (
    <div className="preview-row muted">
      {icon}
      <span>{idleMessage}</span>
    </div>
  );
}
