import { useState } from "react";
import { useT } from "../lib/i18n";

export function SettingsQuietNotice({
  title,
  body,
  detail,
  action,
}: {
  title: string;
  body?: string;
  detail?: string | null;
  action?: { label: string; onClick: () => void };
}) {
  const t = useT();
  const [showDetail, setShowDetail] = useState(false);
  const hasDetail = Boolean(detail && detail !== title && detail !== body);

  return (
    <div className="settings-quiet-notice" role="status">
      <span className="settings-quiet-notice-dot" aria-hidden="true" />
      <div className="settings-quiet-notice-body">
        <strong>{title}</strong>
        {body ? <span>{body}</span> : null}
        {hasDetail && showDetail ? <pre>{detail}</pre> : null}
      </div>
      {hasDetail ? (
        <div className="settings-quiet-notice-actions">
          <button
            type="button"
            className="settings-quiet-notice-link"
            aria-expanded={showDetail}
            onClick={() => setShowDetail((open) => !open)}
          >
            {t("common.details")}
          </button>
          {action ? (
            <button type="button" className="settings-quiet-notice-action" onClick={action.onClick}>
              {action.label}
            </button>
          ) : null}
        </div>
      ) : action ? (
        <div className="settings-quiet-notice-actions">
          <button type="button" className="settings-quiet-notice-action" onClick={action.onClick}>
            {action.label}
          </button>
        </div>
      ) : null}
    </div>
  );
}
