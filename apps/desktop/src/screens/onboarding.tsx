// Onboarding screen — the 4-step first-launch wizard. Extracted from
// App.tsx (B13 Phase C). Owns the step content metadata; every input
// state (folders / YouTube channels / model choices) lives
// in the host so reloading the app preserves the in-progress wizard
// position.

import { ArrowLeft, ArrowRight, FastForward } from "lucide-react";
import type {
  ApiStatus,
  OnboardingYoutubeChannel,
} from "../lib/types";
import { useT } from "../lib/i18n";
import { InlineNotice } from "../components/leaf";
import {
  AccessibilityPermissionCallout,
  OnboardingFolderPicker,
  OnboardingYoutubePicker,
} from "../components/onboarding-pickers";
import { LogoLockup } from "../components/brand";

// Copy is keyed by step index into the i18n catalog (onboarding.step{n}.*).
const onboardingSteps = [
  {
    titleKey: "onboarding.step0.title",
    kickerKey: "onboarding.step0.kicker",
    actionKey: "onboarding.step0.action",
  },
  {
    titleKey: "onboarding.step1.title",
    kickerKey: "onboarding.step1.kicker",
    actionKey: "onboarding.continue",
  },
  {
    titleKey: "onboarding.step2.title",
    kickerKey: "onboarding.step2.kicker",
    actionKey: "onboarding.continue",
  },
  {
    titleKey: "onboarding.step3.title",
    kickerKey: "onboarding.step3.kicker",
    actionKey: "onboarding.startIndexing",
  },
] as const;

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
  const current = onboardingSteps[step];
  const finalStep = step === onboardingSteps.length - 1;
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

  return (
    <div className="onb">
      <div className="onb-body">
        <div className="onb-card">
          <div className="onb-dots" aria-label={t("onboarding.dotsAria")}>
            {onboardingSteps.map((item, index) => (
              <span
                key={item.titleKey}
                className={`onb-dot ${index === step ? "active" : ""}`}
              />
            ))}
          </div>
          <div className="onb-brand">
            <LogoLockup />
          </div>
          <h1 className="onb-title">{t(current.titleKey)}</h1>
          <p className="onb-subtitle muted">{t(current.kickerKey)}</p>

          {step === 0 ? <AccessibilityPermissionCallout /> : null}

          {step === 1 ? (
            <OnboardingFolderPicker folders={folders} setFolders={setFolders} />
          ) : null}

          {step === 2 ? (
            <OnboardingYoutubePicker
              channels={youtubeChannels}
              setChannels={setYoutubeChannels}
            />
          ) : null}

          {step === 3 ? (
            <>
              <div className="onboarding-model-stack">
                <div>
                  <strong>{t("onboarding.model.asrTitle")}</strong>
                  <span>{t("onboarding.model.asrDesc")}</span>
                </div>
                <div>
                  <strong>{t("onboarding.model.embeddingTitle")}</strong>
                  <span>{t("onboarding.model.embeddingDesc")}</span>
                </div>
                <div>
                  <strong>{t("onboarding.model.connectionsTitle")}</strong>
                  <span>{t("onboarding.model.connectionsDesc")}</span>
                </div>
              </div>
              {modelDownloadState.error ? (
                <InlineNotice tone="error" message={modelDownloadState.error} />
              ) : null}
            </>
          ) : null}

          <div className="onb-actions">
            {step > 0 ? (
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setStep(step - 1)}
              >
                <ArrowLeft size={15} />
                <span>{t("common.back")}</span>
              </button>
            ) : null}
            <button
              type="button"
              className="btn btn-primary"
              disabled={finalActionDisabled}
              onClick={() => {
                if (step === onboardingSteps.length - 1) {
                  onDone();
                } else {
                  setStep(step + 1);
                }
              }}
            >
              <span>{finalStep ? finalActionLabel : t(current.actionKey)}</span>
              <ArrowRight size={16} />
            </button>
            {step > 0 && step < onboardingSteps.length - 1 ? (
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setStep(step + 1)}
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
