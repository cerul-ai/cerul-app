// Onboarding sub-components: hotkey demo + Accessibility callout,
// folder picker (with the "Scan common locations" CTA added in PR #4
// A2), and YouTube channel picker with inline validation. Extracted
// from App.tsx (B13 Phase B).

import {
  Check,
  Command,
  Folder,
  Loader2,
  Wrench,
  X,
  Youtube,
  Option,
} from "lucide-react";
import { useState } from "react";
import { uniqueStrings } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { OnboardingYoutubeChannel, ValidationState } from "../lib/types";
import {
  uniqueYoutubeChannels,
  validateHttpUrl,
  waitForValidationFrame,
  youtubeChannelFromUrl,
} from "../lib/validation";
import { SourcePreview } from "./source-preview";
import { invokeHostCommand, openDialog } from "../lib/desktopHost";

async function openAccessibilitySettings() {
  try {
    await invokeHostCommand("open_accessibility_settings");
  } catch (error) {
    console.warn("failed to open Accessibility settings", error);
  }
}

export function AccessibilityPermissionCallout() {
  const t = useT();
  // The ⌥+Space shortcut card now lives in the welcome step (handoff design);
  // this callout carries only the functional macOS Accessibility CTA.
  return (
    <div className="permission-callout">
      <Option size={18} />
      <span>
        <strong>{t("onboarding.accessibility.title")}</strong>
        <small>{t("onboarding.accessibility.body")}</small>
      </span>
      <button
        className="btn btn-ghost accent sm"
        type="button"
        onClick={openAccessibilitySettings}
      >
        {t("onboarding.accessibility.openSettings")}
      </button>
    </div>
  );
}

const COMMON_LOCATIONS = ["~/Movies", "~/Downloads", "~/Desktop", "~/Documents"];

export function OnboardingFolderPicker({
  folders,
  setFolders,
}: {
  folders: string[];
  setFolders: (folders: string[]) => void;
}) {
  const t = useT();
  async function chooseFolders() {
    const selected = await openDialog({ directory: true, multiple: true }).catch(() => null);
    const picked = Array.isArray(selected)
      ? selected
      : typeof selected === "string"
        ? [selected]
        : [];
    if (picked.length > 0) {
      setFolders(uniqueStrings([...folders, ...picked]));
    }
  }

  function addCommonLocations() {
    setFolders(uniqueStrings([...folders, ...COMMON_LOCATIONS]));
  }

  function removeFolder(path: string) {
    setFolders(folders.filter((folder) => folder !== path));
  }

  const commonLocationsAdded = COMMON_LOCATIONS.every((location) =>
    folders.includes(location),
  );

  return (
    <div className="onboarding-picker">
      <button className="btn btn-secondary" type="button" onClick={chooseFolders}>
        <Folder size={18} />
        <span>{t("onboarding.folder.choose")}</span>
      </button>
      <button
        className="btn btn-secondary"
        type="button"
        onClick={addCommonLocations}
        disabled={commonLocationsAdded}
        title={t("onboarding.folder.commonTooltip")}
      >
        <Wrench size={18} />
        <span>
          {commonLocationsAdded
            ? t("onboarding.folder.commonAdded")
            : t("onboarding.folder.scanCommon")}
        </span>
      </button>
      {folders.length > 0 ? (
        <div
          className="row gap-2"
          aria-label={t("onboarding.folder.chipsAria")}
          /* full grid row inside .onboarding-picker; chips wrap, centered card */
          style={{ gridColumn: "1 / -1", flexWrap: "wrap", justifyContent: "center" }}
        >
          {folders.map((folder) => (
            <button
              key={folder}
              className="chip neutral"
              type="button"
              onClick={() => removeFolder(folder)}
              aria-label={t("onboarding.folder.removeChipAria", { folder })}
            >
              <span className="clamp1 mono">{folder}</span>
              <X size={13} />
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function OnboardingYoutubePicker({
  channels,
  setChannels,
}: {
  channels: OnboardingYoutubeChannel[];
  setChannels: (channels: OnboardingYoutubeChannel[]) => void;
}) {
  const t = useT();
  // Starts empty on purpose: a prefilled real channel could get added by a
  // user just clicking through. The example URL lives in the placeholder.
  const [url, setUrl] = useState("");
  const [validation, setValidation] = useState<ValidationState>({
    status: "idle",
    message: null,
  });

  function updateUrl(value: string) {
    setUrl(value);
    setValidation({ status: "idle", message: null });
  }

  async function validateAndAddChannel() {
    setValidation({ status: "validating", message: null });
    await waitForValidationFrame();

    const result = validateHttpUrl(url, t, ["youtube.com", "youtu.be"]);
    if (!result.ok) {
      setValidation({ status: "error", message: result.message });
      return;
    }

    const channel = youtubeChannelFromUrl(url, t);
    setChannels(uniqueYoutubeChannels([...channels, channel]));
    setValidation({
      status: "valid",
      message: t("onboarding.youtube.readyMessage", { name: channel.name }),
    });
  }

  function removeChannel(urlToRemove: string) {
    setChannels(channels.filter((channel) => channel.url !== urlToRemove));
  }

  return (
    <div className="col gap-3">
      <label className="search-wrap">
        <Youtube size={18} />
        <input
          className="search-input"
          value={url}
          onChange={(event) => updateUrl(event.currentTarget.value)}
          placeholder={t("onboarding.youtube.urlPlaceholder")}
        />
      </label>
      <button
        className="btn btn-ghost accent sm"
        type="button"
        onClick={() => void validateAndAddChannel()}
        disabled={!url.trim() || validation.status === "validating"}
      >
        {validation.status === "validating" ? <Loader2 size={15} /> : <Check size={15} />}
        <span>
          {validation.status === "validating"
            ? t("common.validating")
            : t("onboarding.youtube.validate")}
        </span>
      </button>
      {/* Preview only appears once a link is being validated — the idle
          placeholder card was just clutter on the first-run step. */}
      {validation.status !== "idle" ? (
        <SourcePreview
          icon={<Youtube size={19} />}
          initials="YT"
          title={t("source.preview.youtubeTitle")}
          validation={validation}
          idleMessage={t("source.preview.youtubeIdle")}
          validDetail={t("onboarding.youtube.previewValidDetail")}
        />
      ) : null}
      {channels.length > 0 ? (
        <div
          className="youtube-channel-list"
          aria-label={t("onboarding.youtube.listAria")}
        >
          {channels.map((channel) => (
            <button
              key={channel.url}
              className="youtube-channel-card"
              type="button"
              onClick={() => removeChannel(channel.url)}
              aria-label={t("onboarding.youtube.removeAria", { name: channel.name })}
              /* .youtube-channel-card has no button reset; keep app font + pointer */
              style={{ font: "inherit", cursor: "pointer" }}
            >
              <span className="chip neutral" aria-hidden="true">
                {channel.name.slice(0, 2).toUpperCase()}
              </span>
              <span className="col grow" style={{ textAlign: "left" }}>
                <strong className="youtube-channel-card__title">{channel.name}</strong>
                <small className="youtube-channel-card__meta">{channel.subscribers}</small>
              </span>
              <X size={14} />
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
