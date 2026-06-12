// Add Source modal dialog. Extracted from App.tsx (B13 Phase D).
//
// The dialog owns its own form state and validation, but every cross-
// component action runs through the host-supplied onAddSource so the
// dialog never directly hits the API.

import {
  AlertCircle,
  Check,
  Clapperboard,
  FileVideo,
  Folder,
  Loader2,
  Plus,
  Podcast,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState } from "react";
import * as api from "../lib/api";
import { errorMessage, uniqueStrings } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { RequestConfirm, ValidationState } from "../lib/types";
import {
  addSourceDisabled,
  classifyWebVideoUrl,
  type WebVideoClassification,
  validateHttpUrl,
  waitForValidationFrame,
} from "../lib/validation";
import { SourcePreview } from "../components/source-preview";
import { openDialog } from "../lib/desktopHost";
import { useEscapeToClose } from "../lib/use-dismissable";

type SourceTabId = "folder" | "file" | "youtube" | "podcast";
type SourceMode = "local" | "network";

const sourceTabs: {
  id: SourceTabId;
  mode: SourceMode;
  icon: LucideIcon;
  labelKey: string;
}[] = [
  { id: "folder", mode: "local", icon: Folder, labelKey: "addSource.tab.folder" },
  { id: "file", mode: "local", icon: FileVideo, labelKey: "addSource.tab.file" },
  { id: "youtube", mode: "network", icon: Clapperboard, labelKey: "addSource.tab.youtube" },
  { id: "podcast", mode: "network", icon: Podcast, labelKey: "addSource.tab.podcast" },
];

export function AddSourceDialog({
  onClose,
  onAddSource,
  requestConfirm,
}: {
  onClose: () => void;
  onAddSource: (type: string, config: Record<string, unknown>) => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [mode, setMode] = useState<SourceMode>("local");
  const [tab, setTab] = useState<SourceTabId>("folder");
  const [folderPath, setFolderPath] = useState("");
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [youtubeUrl, setYoutubeUrl] = useState("");
  const [webVideoPreview, setWebVideoPreview] = useState<WebVideoClassification | null>(null);
  const [rssUrl, setRssUrl] = useState("");
  const [rssMax, setRssMax] = useState(25);
  const [rssPreview, setRssPreview] = useState<api.RssSourcePreview | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [youtubeValidation, setYoutubeValidation] = useState<ValidationState>({
    status: "idle",
    message: null,
  });
  const [rssValidation, setRssValidation] = useState<ValidationState>({
    status: "idle",
    message: null,
  });
  useEscapeToClose(onClose);
  const visibleTabs = sourceTabs.filter((item) => item.mode === mode);

  function chooseMode(nextMode: SourceMode) {
    setMode(nextMode);
    if (nextMode === "local" && (tab === "youtube" || tab === "podcast")) {
      setTab("folder");
    }
    if (nextMode === "network" && (tab === "folder" || tab === "file")) {
      setTab("youtube");
    }
  }

  async function chooseFolder() {
    const selected = await openDialog({ directory: true, multiple: false }).catch(() => null);
    if (typeof selected === "string") {
      setFolderPath(selected);
    }
  }

  async function chooseFiles() {
    const selected = await openDialog({
      directory: false,
      multiple: true,
      filters: [
        { name: "Video", extensions: ["mp4", "mkv", "webm", "mov", "m4v"] },
      ],
    }).catch(() => null);
    const picked = Array.isArray(selected)
      ? selected.filter((value): value is string => typeof value === "string")
      : typeof selected === "string"
        ? [selected]
        : [];
    if (picked.length > 0) {
      setFilePaths((existing) => uniqueStrings([...existing, ...picked]));
    }
  }

  function removeFilePath(path: string) {
    setFilePaths((existing) => existing.filter((value) => value !== path));
  }

  function updateYoutubeUrl(value: string) {
    setYoutubeUrl(value);
    setYoutubeValidation({ status: "idle", message: null });
    setWebVideoPreview(null);
  }

  function updateRssUrl(value: string) {
    setRssUrl(value);
    setRssValidation({ status: "idle", message: null });
    setRssPreview(null);
  }

  async function validateYoutubeUrl(value = youtubeUrl) {
    setYoutubeValidation({ status: "validating", message: null });
    setWebVideoPreview(null);
    await waitForValidationFrame();

    const result = classifyWebVideoUrl(value, t);
    if (!result.ok) {
      setYoutubeValidation({ status: "error", message: result.message });
      return null;
    }

    setWebVideoPreview(result);
    setYoutubeValidation({
      status: "valid",
      message: t("addSource.youtube.validMessage", {
        hostname: result.hostname,
        platform: t(`addSource.webVideo.platform.${result.platform}`),
      }),
    });
    return result;
  }

  async function validateRssUrl(value = rssUrl) {
    setRssValidation({ status: "validating", message: null });
    await waitForValidationFrame();

    const result = validateHttpUrl(value, t);
    if (!result.ok) {
      setRssValidation({ status: "error", message: result.message });
      return false;
    }

    try {
      const preview = await api.previewRssSource(value.trim());
      setRssPreview(preview);
      setRssValidation({
        status: "valid",
        message: t("addSource.podcast.validMessage", { hostname: result.hostname }),
      });
      return true;
    } catch (previewError) {
      setRssPreview(null);
      setRssValidation({ status: "error", message: errorMessage(previewError) });
      return false;
    }
  }

  async function submit() {
    setIsSaving(true);
    setError(null);
    try {
      if (tab === "folder") {
        if (!folderPath.trim()) {
          setError(t("addSource.folder.errorEmpty"));
          return;
        }
        await onAddSource("folder_video", { path: folderPath });
      } else if (tab === "file") {
        if (filePaths.length === 0) {
          setError(t("addSource.file.errorEmpty"));
          return;
        }
        // Each file becomes its own source so each item is indexed independently.
        for (const path of filePaths) {
          await onAddSource("file_video", { path });
        }
      } else if (tab === "youtube") {
        const preview = await validateYoutubeUrl();
        if (!preview) {
          return;
        }
        if (preview.sourceKind === "author") {
          const confirmed = await requestConfirm({
            title: t("addSource.webVideo.confirmAuthor.title"),
            body: t("addSource.webVideo.confirmAuthor.body", {
              platform: t(`addSource.webVideo.platform.${preview.platform}`),
              hostname: preview.hostname,
            }),
            confirmLabel: t("addSource.webVideo.confirmAuthor.confirm"),
          });
          if (!confirmed) {
            return;
          }
        }
        await onAddSource("web_video", {
          url: preview.url,
          platform: preview.platform,
          source_kind: preview.sourceKind,
          max_videos: preview.sourceKind === "author" ? 0 : 1,
        });
      } else {
        if (!(await validateRssUrl())) {
          return;
        }
        await onAddSource("rss_podcast", { url: rssUrl, max_episodes: rssMax });
      }
      onClose();
    } catch (saveError) {
      setError(errorMessage(saveError));
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <section
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-source-title"
        style={{ maxWidth: 560 }}
      >
        <header className="dhead">
          <div>
            <p className="page-eyebrow">{t("addSource.eyebrow")}</p>
            <h2 id="add-source-title" className="dtitle">
              {t("addSource.title")}
            </h2>
          </div>
          <button
            className="btn-icon"
            type="button"
            onClick={onClose}
            aria-label={t("common.close")}
          >
            <X size={18} />
          </button>
        </header>
        <div className="dbody">
          <div className="source-mode-tabs" role="tablist" aria-label={t("addSource.mode.aria")}>
            {(["local", "network"] as const).map((modeId) => (
              <button
                key={modeId}
                type="button"
                role="tab"
                aria-selected={mode === modeId}
                className={mode === modeId ? "selected" : ""}
                onClick={() => chooseMode(modeId)}
              >
                <span>{t(`addSource.mode.${modeId}`)}</span>
                <small>{t(`addSource.mode.${modeId}.desc`)}</small>
              </button>
            ))}
          </div>
          <div className="type-grid" role="tablist">
            {visibleTabs.map(({ id, icon: TabIcon, labelKey }) => {
              return (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={tab === id}
                  className={`type-card${tab === id ? " selected" : ""}`}
                  onClick={() => setTab(id)}
                >
                  <TabIcon size={18} />
                  <span>{t(labelKey)}</span>
                </button>
              );
            })}
          </div>
          {tab === "folder" ? (
            <FolderTab path={folderPath} setPath={setFolderPath} onChooseFolder={chooseFolder} />
          ) : null}
          {tab === "file" ? (
            <FileTab paths={filePaths} onChooseFiles={chooseFiles} onRemove={removeFilePath} />
          ) : null}
          {tab === "youtube" ? (
            <YoutubeTab
              url={youtubeUrl}
              setUrl={updateYoutubeUrl}
              preview={webVideoPreview}
              validation={youtubeValidation}
              onValidate={() => void validateYoutubeUrl()}
            />
          ) : null}
          {tab === "podcast" ? (
            <PodcastTab
              url={rssUrl}
              setUrl={updateRssUrl}
              max={rssMax}
              setMax={setRssMax}
              validation={rssValidation}
              preview={rssPreview}
              onValidate={() => void validateRssUrl()}
            />
          ) : null}
          {error ? (
            <div className="field-error" role="alert">
              <AlertCircle size={15} />
              <span>{error}</span>
            </div>
          ) : null}
        </div>
        <footer className="dfoot">
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void submit()}
            disabled={
              isSaving ||
              addSourceDisabled(
                tab,
                folderPath,
                filePaths,
                youtubeUrl,
                rssUrl,
                youtubeValidation,
                rssValidation,
              )
            }
          >
            {isSaving ? <Loader2 size={16} /> : <Plus size={16} />}
            <span>{isSaving ? t("addSource.adding") : t("addSource.title")}</span>
          </button>
        </footer>
      </section>
    </div>
  );
}

function FolderTab({
  path,
  setPath,
  onChooseFolder,
}: {
  path: string;
  setPath: (path: string) => void;
  onChooseFolder: () => void;
}) {
  const t = useT();
  return (
    <div className="col gap-3">
      <button className="btn btn-secondary block" type="button" onClick={onChooseFolder}>
        <Folder size={18} />
        <span>{t("onboarding.folder.choose")}</span>
      </button>
      <label className="field-label">
        {t("addSource.folder.pathLabel")}
        <input
          className="input mono"
          value={path}
          onChange={(event) => setPath(event.currentTarget.value)}
          placeholder={t("addSource.folder.pathPlaceholder")}
        />
      </label>
      <p className="field-hint">{t("addSource.folder.helper")}</p>
    </div>
  );
}

function FileTab({
  paths,
  onChooseFiles,
  onRemove,
}: {
  paths: string[];
  onChooseFiles: () => void;
  onRemove: (path: string) => void;
}) {
  const t = useT();
  return (
    <div className="col gap-3">
      <button className="btn btn-secondary block" type="button" onClick={onChooseFiles}>
        <FileVideo size={18} />
        <span>
          {paths.length === 0
            ? t("addSource.file.chooseEmpty")
            : t("addSource.file.chooseMore")}
        </span>
      </button>
      {paths.length > 0 ? (
        <div className="row gap-2" aria-label={t("addSource.file.chipsAria")} style={{ flexWrap: "wrap" }}>
          {paths.map((path) => (
            <button
              key={path}
              className="chip neutral"
              type="button"
              onClick={() => onRemove(path)}
              aria-label={t("addSource.file.removeChipAria", { path })}
            >
              <span className="clamp1 mono">{path}</span>
              <X size={13} />
            </button>
          ))}
        </div>
      ) : null}
      <p className="field-hint">{t("addSource.file.helper")}</p>
    </div>
  );
}

function YoutubeTab({
  url,
  setUrl,
  preview,
  validation,
  onValidate,
}: {
  url: string;
  setUrl: (url: string) => void;
  preview: WebVideoClassification | null;
  validation: ValidationState;
  onValidate: () => void;
}) {
  const t = useT();
  const initials = preview?.platform === "bilibili" ? "BI" : "YT";
  const validDetail =
    preview?.sourceKind === "author"
      ? t("addSource.webVideo.validDetailAuthor")
      : t("addSource.webVideo.validDetailSingle");
  return (
    <div className="col gap-3">
      <label className="field-label">
        {t("addSource.youtube.urlLabel")}
        <input
          className={`input mono${validation.status === "error" ? " error" : ""}`}
          value={url}
          onChange={(event) => setUrl(event.currentTarget.value)}
          placeholder={t("addSource.youtube.urlPlaceholder")}
        />
      </label>
      <button
        className="btn btn-ghost accent sm"
        type="button"
        onClick={onValidate}
        disabled={!url.trim() || validation.status === "validating"}
      >
        {validation.status === "validating" ? <Loader2 size={15} /> : <Check size={15} />}
        <span>
          {validation.status === "validating"
            ? t("common.validating")
            : t("addSource.youtube.validate")}
        </span>
      </button>
      <SourcePreview
        icon={<Clapperboard size={19} />}
        initials={initials}
        title={t("source.preview.webVideoTitle")}
        validation={validation}
        idleMessage={t("source.preview.webVideoIdle")}
        validDetail={validDetail}
      />
      <p className="field-hint">{t("addSource.youtube.helper")}</p>
    </div>
  );
}

function PodcastTab({
  url,
  setUrl,
  max,
  setMax,
  validation,
  preview,
  onValidate,
}: {
  url: string;
  setUrl: (url: string) => void;
  max: number;
  setMax: (max: number) => void;
  validation: ValidationState;
  preview: api.RssSourcePreview | null;
  onValidate: () => void;
}) {
  const t = useT();
  return (
    <div className="col gap-3">
      <label className="field-label">
        {t("addSource.podcast.urlLabel")}
        <input
          className={`input mono${validation.status === "error" ? " error" : ""}`}
          value={url}
          onChange={(event) => setUrl(event.currentTarget.value)}
          placeholder={t("addSource.podcast.urlPlaceholder")}
        />
      </label>
      <button
        className="btn btn-ghost accent sm"
        type="button"
        onClick={onValidate}
        disabled={!url.trim() || validation.status === "validating"}
      >
        {validation.status === "validating" ? <Loader2 size={15} /> : <Check size={15} />}
        <span>
          {validation.status === "validating"
            ? t("common.validating")
            : t("addSource.podcast.validate")}
        </span>
      </button>
      <SourcePreview
        icon={<Podcast size={19} />}
        initials="RSS"
        title={preview?.title ?? t("addSource.podcast.previewTitleFallback")}
        validation={validation}
        idleMessage={t("addSource.podcast.previewIdle")}
        validDetail={
          preview
            ? t("addSource.podcast.validDetailWithCount", {
                count: preview.episode_count,
                max,
              })
            : t("addSource.podcast.validDetailMax", { max })
        }
        imageUrl={preview?.image_url ?? null}
      />
      <label className="field-label inline-field">
        {t("addSource.podcast.maxLabel")}
        <input
          className="input"
          type="number"
          min={1}
          value={max}
          onChange={(event) => setMax(Math.max(1, Number(event.currentTarget.value) || 1))}
        />
      </label>
    </div>
  );
}
