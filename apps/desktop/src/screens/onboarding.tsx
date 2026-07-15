// Onboarding screen — the first-launch wizard, redesigned to the
// design_handoff_cerul 3-step narrative: welcome → add a source →
// smart / local processing. The former folder + YouTube steps are merged
// into one "add a source" step; the final step doubles as the smart-
// processing explainer that kicks off indexing. Every input state
// (folders / YouTube channels / model download) lives in the host so
// reloading the app preserves the in-progress wizard position.

import { ArrowLeft, ArrowRight, FastForward, Folder, Search, SlidersHorizontal } from "lucide-react";
import type {
  ApiStatus,
  OnboardingYoutubeChannel,
} from "../lib/types";
import { useT } from "../lib/i18n";
import { BrandMark } from "../components/brand";
import { InlineNotice } from "../components/leaf";
import {
  AccessibilityPermissionCallout,
  OnboardingFolderPicker,
  OnboardingYoutubePicker,
} from "../components/onboarding-pickers";

const STEP_COUNT = 3;

// Step 0 — welcome / shortcut. Logo squircle with steel glow + sheen, a
// two-line headline, and the ⌥+Space shortcut card.
function WelcomeStep() {
  const t = useT();
  return (
    <div className="onb-step onb-welcome">
      <div className="onb-logo-wrap">
        <span className="logo-glow" />
        <BrandMark className="onb-logo-mark" />
      </div>
      <h1 className="onb-h3">{t("onboarding.step0.title")}</h1>
      <p className="onb-lead">{t("onboarding.welcome.body")}</p>
      <div className="onb-shortcut float-card">
        <kbd className="onb-kbd">⌥</kbd>
        <span className="onb-shortcut-plus">+</span>
        <kbd className="onb-kbd">Space</kbd>
        <span className="onb-shortcut-label">{t("onboarding.welcome.shortcut")}</span>
      </div>
      <AccessibilityPermissionCallout />
    </div>
  );
}

// Step 1 — add a source. Floating file-cards illustration drifting above a
// steel-tinted folder, then the real folder + YouTube pickers.
function AddSourceStep({
  folders,
  setFolders,
  youtubeChannels,
  setYoutubeChannels,
}: {
  folders: string[];
  setFolders: (folders: string[]) => void;
  youtubeChannels: OnboardingYoutubeChannel[];
  setYoutubeChannels: (channels: OnboardingYoutubeChannel[]) => void;
}) {
  const t = useT();
  return (
    <div className="onb-step">
      <div className="onb-illo onb-illo-source" aria-hidden="true">
        <span className="onb-file onb-file-l">
          <span className="onb-play" />
        </span>
        <span className="onb-file onb-file-r">
          <span className="onb-play" />
        </span>
        <span className="onb-file onb-file-c">
          <span className="onb-play" />
        </span>
        <span className="onb-folder">
          <BrandMark className="onb-folder-mark" />
        </span>
      </div>
      <h1 className="onb-h3">{t("onboarding.addSource.title")}</h1>
      <p className="onb-lead">{t("onboarding.addSource.body")}</p>
      <div className="onb-source-pickers">
        <OnboardingFolderPicker folders={folders} setFolders={setFolders} />
        <div className="onb-source-sep">
          <span>{t("onboarding.addSource.youtubeHint")}</span>
        </div>
        <OnboardingYoutubePicker
          channels={youtubeChannels}
          setChannels={setYoutubeChannels}
        />
      </div>
    </div>
  );
}

// Step 2 — smart / local processing. A dark video frame → steel arrow → a
// "transcript" card with a scan-line sweep and a 🔒 本地 badge, then the
function SmartStep({
  modelDownloadState,
  apiStatus,
}: {
  modelDownloadState: { status: string; error: string | null };
  apiStatus: ApiStatus;
}) {
  const t = useT();
  const chips = [
    { icon: "🔒", label: t("onboarding.smart.chipLocal") },
    { icon: "⚡", label: t("onboarding.smart.chipSearch") },
    { icon: "⚙", label: t("onboarding.smart.chipTune") },
  ];
  const detail = [
    { Icon: SlidersHorizontal, title: t("onboarding.model.asrTitle"), desc: t("onboarding.model.asrDesc") },
    { Icon: Search, title: t("onboarding.model.embeddingTitle"), desc: t("onboarding.model.embeddingDesc") },
    { Icon: Folder, title: t("onboarding.model.connectionsTitle"), desc: t("onboarding.model.connectionsDesc") },
  ];
  return (
    <div className="onb-step">
      <div className="onb-illo onb-illo-smart" aria-hidden="true">
        <span className="onb-frame">
          <span className="onb-play onb-play-lg" />
        </span>
        <span className="onb-arrow">
          <ArrowRight size={18} />
        </span>
        <span className="onb-transcript float-card">
          <span className="onb-tline" />
          <span className="onb-tline hot" />
          <span className="onb-tline" />
          <span className="onb-tline short" />
          <span className="onb-scan" />
          <span className="onb-local-badge">🔒 {t("onboarding.smart.chipLocal")}</span>
        </span>
      </div>
      <h1 className="onb-h3">{t("onboarding.smart.title")}</h1>
      <p className="onb-lead">{t("onboarding.smart.body")}</p>
      <div className="onb-chips">
        {chips.map((chip) => (
          <span key={chip.label} className="onb-chip">
            <span aria-hidden="true">{chip.icon}</span>
            {chip.label}
          </span>
        ))}
      </div>
      <div className="onb-detail-stack">
        {detail.map(({ Icon, title, desc }) => (
          <div key={title} className="onb-detail-row">
            <span className="onb-detail-icon"><Icon size={16} /></span>
            <span className="onb-detail-text">
              <strong>{title}</strong>
              <span>{desc}</span>
            </span>
          </div>
        ))}
      </div>
      {apiStatus !== "online" && modelDownloadState.status !== "error" ? (
        <p className="onb-connecting mono">
          <span className="onb-connecting-dot" aria-hidden="true" />
          {t("onboarding.smart.connecting")}
        </p>
      ) : null}
      {modelDownloadState.error ? (
        <InlineNotice tone="error" message={modelDownloadState.error} />
      ) : null}
    </div>
  );
}

export function Onboarding({
  step,
  setStep,
  apiStatus,
  folders,
  setFolders,
  youtubeChannels,
  setYoutubeChannels,
  modelDownloadState,
  onDone,
}: {
  step: number;
  setStep: (step: number) => void;
  apiStatus: ApiStatus;
  folders: string[];
  setFolders: (folders: string[]) => void;
  youtubeChannels: OnboardingYoutubeChannel[];
  setYoutubeChannels: (channels: OnboardingYoutubeChannel[]) => void;
  modelDownloadState: {
    status: "idle" | "saving_sources" | "downloading" | "error";
    error: string | null;
  };
  onDone: () => void;
}) {
  const t = useT();
  const clamped = Math.min(Math.max(step, 0), STEP_COUNT - 1);
  const finalStep = clamped === STEP_COUNT - 1;
  const selectedSourceCount = folders.length + youtubeChannels.length;

  const finalActionLabel =
    modelDownloadState.status === "saving_sources"
      ? t(
          selectedSourceCount === 1
            ? "onboarding.final.addingOne"
            : "onboarding.final.addingOther",
          { count: selectedSourceCount },
        )
    : modelDownloadState.status === "downloading"
      ? t("onboarding.final.savingDefaults")
      : t("onboarding.startIndexing");
  const finalActionDisabled =
    finalStep &&
    (apiStatus !== "online" ||
      modelDownloadState.status === "saving_sources" ||
      modelDownloadState.status === "downloading");

  const primaryLabel = finalStep
    ? finalActionLabel
    : clamped === 0
      ? t("onboarding.welcome.start")
      : t("onboarding.continue");

  return (
    <div className="onb">
      <div className="onb-body">
        <div className="onb-card onb-card-wide">
          {/* Progress — three inert dots with a steel pill that springs to the active step. */}
          <div className="onb-progress" role="img" aria-label={t("onboarding.dotsAria")}>
            <span className="onb-progress-pill" style={{ left: `${clamped * 20}px` }} />
            {Array.from({ length: STEP_COUNT }).map((_, index) => (
              <i key={index} className="onb-progress-dot" />
            ))}
          </div>

          <div className="onb-stage">
            {clamped === 0 ? <WelcomeStep /> : null}
            {clamped === 1 ? (
              <AddSourceStep
                folders={folders}
                setFolders={setFolders}
                youtubeChannels={youtubeChannels}
                setYoutubeChannels={setYoutubeChannels}
              />
            ) : null}
            {clamped === 2 ? (
              <SmartStep modelDownloadState={modelDownloadState} apiStatus={apiStatus} />
            ) : null}
          </div>

          <div className="onb-actions">
            {clamped > 0 ? (
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setStep(clamped - 1)}
              >
                <ArrowLeft size={15} />
                <span>{t("common.back")}</span>
              </button>
            ) : null}
            <button
              type="button"
              className="btn btn-primary lg"
              disabled={finalActionDisabled}
              onClick={() => {
                if (finalStep) {
                  onDone();
                } else {
                  setStep(clamped + 1);
                }
              }}
            >
              <span>{primaryLabel}</span>
              <ArrowRight size={16} />
            </button>
            {clamped === 0 ? (
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setStep(STEP_COUNT - 1)}
              >
                <span>{t("onboarding.welcome.later")}</span>
                <FastForward size={15} />
              </button>
            ) : !finalStep ? (
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setStep(clamped + 1)}
              >
                <span>{t("onboarding.skip")}</span>
                <FastForward size={15} />
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
