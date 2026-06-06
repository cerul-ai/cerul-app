// App-wide banner shown when Cerul Core (the API server) is unreachable.
// Extracted from App.tsx (B13 Phase B).
//
// Pure props-driven. The host supplies the current ApiStatus and last
// error. The banner shows a calm "starting up..." spinner for the first
// 10 seconds, then escalates to "unresponsive" with a Restart button.
// Retries and restarts run through the host-provided onAction callback
// so the banner does not need to know about desktop shell commands.

import { AlertTriangle, Loader2, RefreshCcw } from "lucide-react";
import { useEffect, useState } from "react";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { ApiStatus, CoreBannerAction } from "../lib/types";

export function CoreBanner({
  status,
  error,
  onAction,
}: {
  status: ApiStatus;
  error: string | null;
  onAction: () => Promise<void> | void;
}) {
  const t = useT();
  const [elapsedMs, setElapsedMs] = useState(0);
  const [actionState, setActionState] = useState<{
    status: "idle" | "running" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const unresponsive = elapsedMs >= 10_000 || status === "error";
  const action: CoreBannerAction = unresponsive ? "restart" : "retry";
  const message = unresponsive
    ? t("coreBanner.unresponsive")
    : t("coreBanner.starting");

  useEffect(() => {
    setElapsedMs(0);
    setActionState({ status: "idle", message: null });
    const startedAt = Date.now();
    const interval = window.setInterval(() => {
      setElapsedMs(Date.now() - startedAt);
    }, 500);
    return () => window.clearInterval(interval);
  }, [status, error]);

  async function runAction() {
    setActionState({ status: "running", message: null });
    try {
      await onAction();
      setActionState({ status: "idle", message: null });
    } catch (actionError) {
      setActionState({ status: "error", message: errorMessage(actionError) });
    }
  }

  return (
    <div className={unresponsive ? "core-banner unresponsive" : "core-banner"} role="status">
      <span>
        {unresponsive ? <AlertTriangle size={15} /> : <Loader2 size={15} />}
        <span>
          <strong>{message}</strong>
          {unresponsive && error ? <small>{error}</small> : null}
          {actionState.message ? <small>{actionState.message}</small> : null}
        </span>
      </span>
      <button
        type="button"
        className={
          unresponsive
            ? "btn btn-danger solid sm"
            : "btn btn-secondary sm"
        }
        disabled={actionState.status === "running"}
        onClick={() => void runAction()}
      >
        {actionState.status === "running" ? (
          <Loader2 size={14} />
        ) : (
          <RefreshCcw size={14} />
        )}
        <span>
          {actionState.status === "running"
            ? action === "restart"
              ? t("coreBanner.restarting")
              : t("coreBanner.retrying")
            : action === "restart"
              ? t("coreBanner.restart")
              : t("coreBanner.retry")}
        </span>
      </button>
    </div>
  );
}
