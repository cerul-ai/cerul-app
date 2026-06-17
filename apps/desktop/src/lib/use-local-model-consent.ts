// Drives the first-run on-device-model consent dialog + download progress.
// Calls the core (prepareLocalModels + poll localPrepareStatus); the same state
// backs the dialog, the sidebar pill and the ready toast.

import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";

type State = {
  show: boolean;
  minimized: boolean;
  paused: boolean;
  ready: boolean;
  capability: api.LocalModelCapability | null;
  download: api.LocalPrepareStatus | null;
};

const IDLE: State = {
  show: false,
  minimized: false,
  paused: false,
  ready: false,
  capability: null,
  download: null,
};

export function useLocalModelConsent(args: { trigger: boolean; apiOnline: boolean }) {
  const [s, setS] = useState<State>(IDLE);
  const timer = useRef<number | null>(null);
  const clear = () => {
    if (timer.current) {
      window.clearInterval(timer.current);
      timer.current = null;
    }
  };

  // Open the dialog once when the host asks (first-run gate). Fetch capability
  // first and only prompt machines that can actually run on-device well —
  // otherwise staying on cloud silently beats a dialog that recommends cloud
  // back. The capability fetch simply rejects until the core route exists, so
  // this never fires prematurely.
  useEffect(() => {
    if (!args.trigger || s.show || s.download || s.ready) {
      return;
    }
    let cancelled = false;
    api
      .localModelCapability()
      .then((capability) => {
        if (!cancelled && capability.can_run_local) {
          setS((p) => ({ ...p, show: true, capability }));
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [args.trigger, s.show, s.download, s.ready]);

  const pollStatus = useCallback(() => {
    api
      .localPrepareStatus()
      .then((download) => {
        setS((p) => {
          if (download.phase === "ready") {
            clear();
            return { ...p, download, show: false, minimized: false, paused: false, ready: true };
          }
          return { ...p, download, paused: false };
        });
      })
      .catch(() => undefined);
  }, []);

  // Re-attach to an in-flight download after a relaunch — or whenever the user
  // was already prompted, so the first-run effect above never fires. The
  // sidecar keeps downloading in the background regardless of the UI; without
  // this the rail pill (which needs the in-memory poller) would never appear
  // and the download stays invisible. Show only the pill (minimized), never
  // re-pop the consent dialog.
  useEffect(() => {
    if (!args.apiOnline || s.show || s.download || s.ready || timer.current) {
      return;
    }
    let cancelled = false;
    api
      .localPrepareStatus()
      .then((download) => {
        if (cancelled || download.phase !== "downloading") {
          return;
        }
        setS((p) => ({ ...p, download, minimized: true }));
        clear();
        timer.current = window.setInterval(pollStatus, 1200);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [args.apiOnline, s.show, s.download, s.ready, pollStatus]);

  const agree = useCallback(() => {
    setS((p) => {
      const cap = p.capability;
      if (!cap) {
        return p;
      }
      // Seed an optimistic "downloading" view; the first poll replaces it with
      // real per-model progress from the core.
      const seed: api.LocalPrepareStatus = {
        phase: "downloading",
        overall_progress: 0,
        done_mb: 0,
        total_mb: cap.total_mb,
        eta_seconds: null,
        active_source: null,
        source_label: null,
        download_bps: null,
        can_pause: false,
        can_cancel: false,
        last_source_error: null,
        last_source: null,
        last_source_label: null,
        last_download_bps: null,
        probes: null,
        models: cap.models.map((m, i) => ({
          id: m.id,
          label: m.label,
          size_mb: m.size_mb,
          status: i === 0 ? "downloading" : "pending",
          progress: 0,
        })),
        error: null,
      };
      return { ...p, download: seed, paused: false };
    });
    api.prepareLocalModels().catch(() => undefined);
    clear();
    timer.current = window.setInterval(pollStatus, 1200);
  }, [pollStatus]);

  const decline = useCallback(() => {
    clear();
    setS((p) => ({ ...p, show: false, paused: false, download: null }));
  }, []);
  const pauseDownload = useCallback(() => {
    clear();
    api
      .cancelLocalModelPrepare()
      .then((download) => setS((p) => ({ ...p, download, paused: true })))
      .catch(() => setS((p) => ({ ...p, paused: true })));
  }, []);
  const resumeDownload = useCallback(() => {
    setS((p) => ({ ...p, paused: false, show: true, minimized: false }));
    api.prepareLocalModels().catch(() => undefined);
    clear();
    timer.current = window.setInterval(pollStatus, 1200);
    pollStatus();
  }, [pollStatus]);
  const cancelDownload = useCallback(() => {
    clear();
    api.cancelLocalModelPrepare().catch(() => undefined);
    setS((p) => ({ ...p, show: false, minimized: false, paused: false, download: null }));
  }, []);
  const background = useCallback(() => setS((p) => ({ ...p, minimized: true })), []);
  const reopen = useCallback(() => setS((p) => ({ ...p, minimized: false })), []);
  const dismissReady = useCallback(() => setS((p) => ({ ...p, ready: false })), []);

  useEffect(() => () => clear(), []);

  return {
    ...s,
    agree,
    decline,
    pauseDownload,
    resumeDownload,
    cancelDownload,
    background,
    reopen,
    dismissReady,
  };
}
