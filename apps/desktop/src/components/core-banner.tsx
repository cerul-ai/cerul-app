// Local-core status UI. Replaces the old in-flow CoreBanner so nothing pushes
// page content around:
//
//   ok           → core online (rail dot green, steady)
//   grace (0–2s) → brief blip; show nothing new (absorbs transient flicker)
//   starting     → rail dot turns amber + "核心启动中…" (quiet, in the rail)
//   unresponsive → (>10s, or hard error) a floating restart toast (overlay)
//
// Timing lives in useCoreStatus so the rail dot and the toast share one
// escalation clock. The toast is rendered at app level and fades in/out via
// the `show` flag — it never participates in content layout.

import { AlertTriangle, Loader2, RefreshCcw } from "lucide-react";
import { useEffect, useState } from "react";
import { errorMessage } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { ApiStatus } from "../lib/types";

export type CoreLevel = "ok" | "grace" | "starting" | "unresponsive";

const GRACE_MS = 2_000;
const UNRESPONSIVE_MS = 10_000;

export function useCoreStatus(status: ApiStatus, error: string | null): CoreLevel {
  const [elapsedMs, setElapsedMs] = useState(0);

  useEffect(() => {
    if (status === "online") {
      setElapsedMs(0);
      return;
    }
    setElapsedMs(0);
    const startedAt = Date.now();
    const interval = window.setInterval(() => {
      setElapsedMs(Date.now() - startedAt);
    }, 500);
    return () => window.clearInterval(interval);
  }, [status, error]);

  if (status === "online") return "ok";
  if (status === "error" || elapsedMs >= UNRESPONSIVE_MS) return "unresponsive";
  if (elapsedMs >= GRACE_MS) return "starting";
  return "grace";
}

export function CoreStatusToast({
  show,
  error,
  onAction,
}: {
  show: boolean;
  error: string | null;
  onAction: () => Promise<void> | void;
}) {
  const t = useT();
  const [actionState, setActionState] = useState<{
    status: "idle" | "running" | "error";
    message: string | null;
  }>({ status: "idle", message: null });

  // Clear any stale action feedback once the toast hides (core recovered).
  useEffect(() => {
    if (!show) {
      setActionState({ status: "idle", message: null });
    }
  }, [show]);

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
    <div
      className="core-toast"
      data-show={show ? "true" : undefined}
      role="status"
      aria-live="polite"
      aria-hidden={show ? undefined : "true"}
      title={error ?? undefined}
    >
      <AlertTriangle size={16} className="ic" aria-hidden="true" />
      <span className="core-toast-text">
        <strong>{t("coreBanner.unresponsive")}</strong>
        {actionState.message ? <small>{actionState.message}</small> : null}
      </span>
      <button
        type="button"
        disabled={actionState.status === "running" || !show}
        onClick={() => void runAction()}
      >
        {actionState.status === "running" ? (
          <Loader2 size={14} className="spin" />
        ) : (
          <RefreshCcw size={14} />
        )}
        <span>
          {actionState.status === "running"
            ? t("coreBanner.retrying")
            : t("coreBanner.retry")}
        </span>
      </button>
    </div>
  );
}
