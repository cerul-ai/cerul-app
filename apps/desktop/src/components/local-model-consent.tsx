// First-run consent + download-progress dialog for on-device models.
// Phase "consent" asks whether to download local models; on agree it flips to
// a "downloading" progress view (the same dialog), backed by the host's live
// prepare status. Mirrors design/Cerul_local-model-flow-proposal.html (屏 A/B).

import { ArrowRight, Check } from "lucide-react";
import { brandAssets } from "../lib/brand";
import { formatDuration } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { LocalModelCapability, LocalPrepareStatus } from "../lib/api";

function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

// The core returns English labels; localize the three known model ids and fall
// back to whatever label the server sent for anything unrecognized.
const KNOWN_MODEL_IDS = new Set(["embed", "asr", "ocr"]);

export function LocalModelConsent({
  capability,
  download,
  onAgree,
  onDecline,
  onBackground,
}: {
  capability: LocalModelCapability | null;
  download: LocalPrepareStatus | null;
  onAgree: () => void;
  onDecline: () => void;
  onBackground: () => void;
}) {
  const t = useT();
  const downloading = download?.phase === "downloading";
  const totalMb = capability?.total_mb ?? download?.total_mb ?? 2100;
  const canLocal = capability?.can_run_local ?? true;

  return (
    <div className="scrim lm-scrim" role="presentation">
      <div className="lm-dialog" role="dialog" aria-modal="true" aria-label={t("localModel.consent.title")}>
        <span className={downloading ? "lm-icon pulse" : "lm-icon"}>
          <img src={brandAssets.appIcon} alt="" />
        </span>

        {downloading && download ? (
          <>
            <h3 className="lm-title">{t("localModel.downloading.title")}</h3>
            <div className="lm-overall">
              <span className="lm-track">
                <span className="lm-fill" style={{ width: `${download.overall_progress}%` }} />
              </span>
              <p className="lm-meta mono">
                {formatSize(download.done_mb)} / {formatSize(download.total_mb)}
                {download.eta_seconds != null ? ` · ${t("home.continueRemaining", { remaining: formatDuration(download.eta_seconds) })}` : ""}
                {" · "}
                <b className="lm-free">$0</b>
              </p>
            </div>
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
            <button type="button" className="btn btn-ghost lm-btn" onClick={onBackground}>
              <span>{t("localModel.downloading.background")}</span>
              <ArrowRight size={15} />
            </button>
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
