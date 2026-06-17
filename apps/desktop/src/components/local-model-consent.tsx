// First-run consent + download-progress dialog for on-device models.
// Phase "consent" asks whether to download local models; on agree it flips to
// a "downloading" progress view (the same dialog), backed by the host's live
// prepare status. Mirrors design/Cerul_local-model-flow-proposal.html (屏 A/B).

import { ArrowRight, Check, Pause, Play, X } from "lucide-react";
import { brandAssets } from "../lib/brand";
import { formatDuration } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { LocalModelCapability, LocalPrepareStatus } from "../lib/api";

function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

function formatSpeed(bps: number | null | undefined): string | null {
  if (!bps || !Number.isFinite(bps) || bps <= 0) {
    return null;
  }
  const mb = bps / 1024 / 1024;
  if (mb >= 1) {
    return `${mb.toFixed(1)} MB/s`;
  }
  return `${Math.max(1, Math.round(bps / 1024))} KB/s`;
}

// The core returns English labels; localize the three known model ids and fall
// back to whatever label the server sent for anything unrecognized.
const KNOWN_MODEL_IDS = new Set(["embed", "asr", "ocr"]);

export function LocalModelConsent({
  capability,
  download,
  paused,
  onAgree,
  onDecline,
  onPause,
  onResume,
  onCancelDownload,
  onBackground,
}: {
  capability: LocalModelCapability | null;
  download: LocalPrepareStatus | null;
  paused?: boolean;
  onAgree: () => void;
  onDecline: () => void;
  onPause: () => void;
  onResume: () => void;
  onCancelDownload: () => void;
  onBackground: () => void;
}) {
  const t = useT();
  const preparing = !!download && download.phase !== "ready";
  const totalMb = capability?.total_mb ?? download?.total_mb ?? 2100;
  const canLocal = capability?.can_run_local ?? true;
  const speed = formatSpeed(download?.download_bps);
  const statusParts = [
    download?.source_label ? t("localModel.downloading.source", { source: download.source_label }) : null,
    speed,
    download?.eta_seconds != null
      ? t("home.continueRemaining", { remaining: formatDuration(download.eta_seconds) })
      : null,
  ].filter(Boolean);

  return (
    <div className="scrim lm-scrim" role="presentation">
      <div className="lm-dialog" role="dialog" aria-modal="true" aria-label={t("localModel.consent.title")}>
        <span className={preparing && !paused ? "lm-icon pulse" : "lm-icon"}>
          <img src={brandAssets.appIcon} alt="" />
        </span>

        {preparing && download ? (
          <>
            <h3 className="lm-title">
              {paused ? t("localModel.downloading.pausedTitle") : t("localModel.downloading.title")}
            </h3>
            <div className="lm-overall">
              <span className="lm-track">
                <span className="lm-fill" style={{ width: `${download.overall_progress}%` }} />
              </span>
              <p className="lm-meta mono">
                {formatSize(download.done_mb)} / {formatSize(download.total_mb)}
                {statusParts.length > 0 ? ` · ${statusParts.join(" · ")}` : ""}
              </p>
            </div>
            {download.last_source_error ? (
              <p className="lm-source-warning">{download.last_source_error}</p>
            ) : null}
            <div className="lm-list">
              {download.models.map((m) => (
                <div key={m.id} className={`lm-row ${m.status}`}>
                  <span className="lm-row-name">
                    <span className="lm-row-dot" />
                    {KNOWN_MODEL_IDS.has(m.id) ? t(`localModel.model.${m.id}`) : m.label}
                  </span>
                  <span className="lm-row-state mono">
                    {m.status === "ready"
                      ? `✓ ${t("localModel.status.ready")}`
                      : m.status === "downloading"
                        ? `↓ ${m.progress}%`
                        : t("localModel.status.pending")}
                  </span>
                </div>
              ))}
            </div>
            <div className="lm-actions">
              {paused ? (
                <button type="button" className="btn btn-primary lm-btn" onClick={onResume}>
                  <Play size={15} />
                  <span>{t("localModel.downloading.resume")}</span>
                </button>
              ) : (
                <button
                  type="button"
                  className="btn btn-secondary lm-btn"
                  disabled={!download.can_pause}
                  onClick={onPause}
                >
                  <Pause size={15} />
                  <span>{t("localModel.downloading.pause")}</span>
                </button>
              )}
              <button type="button" className="btn btn-ghost lm-btn" onClick={onCancelDownload}>
                <X size={15} />
                <span>{t("localModel.downloading.cancel")}</span>
              </button>
              <button type="button" className="btn btn-ghost lm-btn" onClick={onBackground}>
                <span>{t("localModel.downloading.background")}</span>
                <ArrowRight size={15} />
              </button>
            </div>
          </>
        ) : (
          <>
            <h3 className="lm-title">{t("localModel.consent.title")}</h3>
            <p className="lm-desc">{t("localModel.consent.body", { size: formatSize(totalMb) })}</p>
            <div className="lm-props">
              <span className="lm-prop">🔒 {t("localModel.prop.local")}</span>
              <span className="lm-prop">⚡ {t("localModel.prop.free")}</span>
              <span className="lm-prop">✈ {t("localModel.prop.offline")}</span>
            </div>
            {capability ? (
              <div className={canLocal ? "lm-machine ok" : "lm-machine weak"}>
                {canLocal ? <Check size={13} /> : null}
                {canLocal
                  ? t("localModel.machine.ok", { arch: capability.arch, ram: capability.ram_gb })
                  : t("localModel.machine.weak")}
              </div>
            ) : null}
            <div className="lm-actions">
              {canLocal ? (
                <>
                  <button type="button" className="btn btn-primary lm-btn" onClick={onAgree}>
                    <span>{t("localModel.consent.agree")}</span>
                    <span className="lm-size mono">· {formatSize(totalMb)}</span>
                  </button>
                  <button type="button" className="btn btn-ghost lm-btn" onClick={onDecline}>
                    {t("localModel.consent.decline")}
                  </button>
                </>
              ) : (
                <>
                  <button type="button" className="btn btn-primary lm-btn" onClick={onDecline}>
                    {t("localModel.consent.decline")}
                  </button>
                  <button type="button" className="btn btn-ghost lm-btn" onClick={onAgree}>
                    <span>{t("localModel.consent.agree")}</span>
                    <span className="lm-size mono">· {formatSize(totalMb)}</span>
                  </button>
                </>
              )}
            </div>
            <p className="lm-foot">{t("localModel.consent.switchHint")}</p>
          </>
        )}
      </div>
    </div>
  );
}
