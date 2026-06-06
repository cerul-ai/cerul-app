// Confirm dialog. Extracted from App.tsx (B13 Phase D).
//
// Props-driven; the host owns the open/close state via the `request`
// prop (null = closed). resolveConfirm in App.tsx is responsible for
// fulfilling the request's resolve callback.

import { AlertTriangle, Trash2 } from "lucide-react";
import { useEffect } from "react";
import { useT } from "../lib/i18n";
import type { ConfirmRequest } from "../lib/types";

export function ConfirmDialog({
  request,
  onCancel,
  onConfirm,
}: {
  request: ConfirmRequest | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const t = useT();
  useEffect(() => {
    if (!request) {
      return;
    }
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [request, onCancel]);

  if (!request) {
    return null;
  }

  return (
    <div className="scrim" role="presentation" onMouseDown={onCancel}>
      <section
        className="dialog confirm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dhead">
          <span className="confirm-icon" aria-hidden="true">
            <AlertTriangle size={18} />
          </span>
          <div>
            <p className="section-label">{t("confirm.eyebrow")}</p>
            <h2 id="confirm-title" className="dtitle">
              {request.title}
            </h2>
          </div>
        </header>
        <div className="dbody">
          <p className="ddesc">{request.body}</p>
        </div>
        <footer className="dfoot">
          <button className="btn btn-ghost" type="button" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button className="btn btn-danger solid" type="button" onClick={onConfirm}>
            <Trash2 size={16} />
            <span>{request.confirmLabel}</span>
          </button>
        </footer>
      </section>
    </div>
  );
}
