// Drives the first-run on-device-model consent dialog + download progress.
// Two modes:
//   live  — calls the core (prepareLocalModels + poll localPrepareStatus)
//   mock  — simulates a download (fixture harness, for design QA / verification)
// State is shared by the dialog, the sidebar pill and the ready toast.

import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";

const MOCK_CAPABILITY: api.LocalModelCapability = {
  can_run_local: true,
  apple_silicon: true,
  arch: "Apple Silicon",
  ram_gb: 16,
  recommended: "local",
  total_mb: 2100,
  models: [
    { id: "asr", label: "语音转写 · Qwen3-ASR", size_mb: 320 },
    { id: "embed", label: "多模态嵌入 · Qwen3-VL", size_mb: 1500 },
    { id: "ocr", label: "画面文字 · OCR", size_mb: 1000 },
  ],
};

type State = {
  show: boolean;
  minimized: boolean;
  ready: boolean;
  capability: api.LocalModelCapability | null;
  download: api.LocalPrepareStatus | null;
};

const IDLE: State = { show: false, minimized: false, ready: false, capability: null, download: null };

export function useLocalModelConsent(args: { mode: "mock" | "live"; trigger: boolean }) {
  const [s, setS] = useState<State>(IDLE);
  const timer = useRef<number | null>(null);
  const clear = () => {
    if (timer.current) {
      window.clearInterval(timer.current);
      timer.current = null;
    }
  };

  // Open the dialog when the host asks (fixture: a query flag; live: first-run
  // gate decided by the host). Fetch capability in live mode.
  useEffect(() => {
    if (!args.trigger || s.show || s.download || s.ready) {
      return;
    }
    if (args.mode === "mock") {
      setS((p) => ({ ...p, show: true, capability: MOCK_CAPABILITY }));
      return;
    }
    let cancelled = false;
    api
      .localModelCapability()
      .then((capability) => {
        // Only prompt machines that can actually run on-device well — otherwise
        // staying on cloud silently is the better default than a dialog that
        // just recommends cloud back.
        if (!cancelled && capability.can_run_local) {
          setS((p) => ({ ...p, show: true, capability }));
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [args.trigger, args.mode, s.show, s.download, s.ready]);

  const tickMock = useCallback(() => {
    setS((p) => {
      if (!p.download) return p;
      const models = p.download.models.map((m) => ({ ...m }));
      const total = p.download.total_mb;
      let done = 0;
      let advanced = false;
      for (const m of models) {
        if (m.status === "ready") {
          done += m.size_mb;
          continue;
        }
        if (!advanced) {
          m.status = "downloading";
          m.progress = Math.min(100, m.progress + 14);
          done += (m.size_mb * m.progress) / 100;
          if (m.progress >= 100) m.status = "ready";
          advanced = true;
        }
      }
      const allReady = models.every((m) => m.status === "ready");
      const overall = Math.min(100, Math.round((done / total) * 100));
      const download: api.LocalPrepareStatus = {
        ...p.download,
        models,
        done_mb: done,
        overall_progress: overall,
        eta_seconds: allReady ? 0 : Math.max(5, Math.round(((total - done) / total) * 150)),
        phase: allReady ? "ready" : "downloading",
      };
      if (allReady) {
        clear();
        return { ...p, download, show: false, minimized: false, ready: true };
      }
      return { ...p, download };
    });
  }, []);

  const pollLive = useCallback(() => {
    api
      .localPrepareStatus()
      .then((download) => {
        setS((p) => {
          if (download.phase === "ready") {
            clear();
            return { ...p, download, show: false, minimized: false, ready: true };
          }
          return { ...p, download };
        });
      })
      .catch(() => undefined);
  }, []);

  const agree = useCallback(() => {
    const cap = s.capability ?? MOCK_CAPABILITY;
    const seed: api.LocalPrepareStatus = {
      phase: "downloading",
      overall_progress: 0,
      done_mb: 0,
      total_mb: cap.total_mb,
      eta_seconds: 180,
      models: cap.models.map((m, i) => ({
        id: m.id,
        label: m.label,
        size_mb: m.size_mb,
        status: i === 0 ? "downloading" : "pending",
        progress: 0,
      })),
      error: null,
    };
    setS((p) => ({ ...p, download: seed }));
    clear();
    if (args.mode === "mock") {
      timer.current = window.setInterval(tickMock, 450);
    } else {
      api.prepareLocalModels().catch(() => undefined);
      timer.current = window.setInterval(pollLive, 1200);
    }
  }, [args.mode, s.capability, tickMock, pollLive]);

  const decline = useCallback(() => {
    clear();
    setS((p) => ({ ...p, show: false, download: null }));
  }, []);
  const background = useCallback(() => setS((p) => ({ ...p, minimized: true })), []);
  const reopen = useCallback(() => setS((p) => ({ ...p, minimized: false })), []);
  const dismissReady = useCallback(() => setS((p) => ({ ...p, ready: false })), []);

  useEffect(() => () => clear(), []);

  return { ...s, agree, decline, background, reopen, dismissReady };
}
