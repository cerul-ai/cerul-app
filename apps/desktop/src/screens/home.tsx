import {
  Check,
  ChevronRight,
  Command,
  Folder,
  FolderDown,
  Image as ImageIcon,
  Loader2,
  Lock,
  Mic,
  Play,
  Plus,
  Search,
  Sparkles,
  X,
  Youtube,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Fragment, useEffect, useState } from "react";
import type { FormEvent } from "react";
import { BrandMark } from "../components/brand";
import { ItemModalityIcon } from "../components/cards";
import { EmptyState } from "../components/leaf";
import * as api from "../lib/api";
import {
  formatDuration,
  formatHotkeyLabel,
  parseTimestampSeconds,
} from "../lib/formatters";
import {
  isActiveJob,
  isNearEndPosition,
  itemKindLabel,
} from "../lib/items";
import { durationMinutes } from "../lib/library";
import { readLastOpened } from "../lib/last-opened";
import { submitSearchInputOnEnter } from "../lib/route";
import { useT, type TFunction } from "../lib/i18n";
import type { ApiStatus, Item, Source } from "../lib/types";

function formatWeeklyHours(seconds: number) {
  const hours = Math.floor(Math.max(0, seconds) / 3600);
  const minutes = Math.round((Math.max(0, seconds) % 3600) / 60);
  if (hours > 0 && minutes > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (hours > 0) {
    return `${hours}h`;
  }
  return `${minutes}m`;
}

function coreStatusText(status: ApiStatus, t: TFunction): string {
  return status === "connecting" ? t("shell.coreConnecting") : t("shell.coreOffline");
}

function HomeEmptyState({ onAddSource }: { onAddSource: () => void }) {
  const t = useT();
  const [dragOver, setDragOver] = useState(false);
  return (
    <div className="page home-empty">
      <div className="home-empty-head">
        <span className="mono-eyebrow">
          <span className="dot" />
          {t("home.emptyHero.eyebrow")}
        </span>
        <h1 className="home-empty-title">{t("home.emptyHero.title")}</h1>
        <p className="home-empty-body">{t("home.emptyHero.body")}</p>
      </div>

      <div
        className={dragOver ? "drag-zone over" : "drag-zone"}
        onDragOver={(event) => {
          event.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(event) => {
          event.preventDefault();
          setDragOver(false);
          onAddSource();
        }}
      >
        <span className="drag-icon">
          <FolderDown size={22} />
        </span>
        <div className="drag-text">
          <strong>{t("home.emptyHero.dragTitle")}</strong>
          <small>{t("home.emptyHero.dragHint")}</small>
        </div>
        <div className="drag-actions">
          <button className="btn btn-primary" type="button" onClick={onAddSource}>
            <Folder size={16} />
            <span>{t("onboarding.folder.choose")}</span>
          </button>
          <button className="btn btn-secondary" type="button" onClick={onAddSource}>
            <Youtube size={16} />
            <span>{t("home.emptyHero.followYoutube")}</span>
          </button>
        </div>
      </div>
    </div>
  );
}

const FIRST_RUN_EXAMPLE_KEYS: { key: string; icon: LucideIcon; tagKey?: string }[] = [
  { key: "firstRun.example.said", icon: Mic },
  { key: "firstRun.example.shown", icon: ImageIcon, tagKey: "firstRun.tagVisual" },
  { key: "firstRun.example.todo", icon: Sparkles },
];

function firstRunFeatures(t: TFunction, globalHotkey: string) {
  return [
    { icon: Mic, title: t("firstRun.feat.said.title"), desc: t("firstRun.feat.said.desc") },
    { icon: ImageIcon, title: t("firstRun.feat.shown.title"), desc: t("firstRun.feat.shown.desc") },
    {
      icon: Command,
      title: t("firstRun.feat.summon.title"),
      desc: t("firstRun.feat.summon.desc", { hotkey: formatHotkeyLabel(globalHotkey) }),
    },
  ];
}

function FirstRunStepper({ activeIndex }: { activeIndex: number }) {
  const t = useT();
  const labels = [t("firstRun.steps.source"), t("firstRun.steps.index"), t("firstRun.steps.search")];
  return (
    <div className="fr-stepper" role="list" aria-label={t("firstRun.steps.aria")}>
      {labels.map((label, index) => {
        const status = index < activeIndex ? "done" : index === activeIndex ? "active" : "todo";
        return (
          <Fragment key={label}>
            {index > 0 ? (
              <span className={index <= activeIndex ? "fr-conn fill" : "fr-conn"} aria-hidden="true" />
            ) : null}
            <span className={`fr-step ${status}`} role="listitem">
              <span className="fr-mk">{status === "done" ? <Check size={13} /> : index + 1}</span>
              <span className="fr-step-label">{label}</span>
            </span>
          </Fragment>
        );
      })}
    </div>
  );
}

function FirstRunIndexing({ statusLabel, globalHotkey }: { statusLabel: string; globalHotkey: string }) {
  const t = useT();
  const features = firstRunFeatures(t, globalHotkey);
  return (
    <div className="page home-firstrun">
      <div className="fr-indexing">
        <div className="onb-illo onb-illo-source fr-illo" aria-hidden="true">
          <span className="onb-file onb-file-l"><span className="onb-play" /></span>
          <span className="onb-file onb-file-r"><span className="onb-play" /></span>
          <span className="onb-file onb-file-c"><span className="onb-play" /></span>
          <span className="onb-folder">
            <BrandMark className="onb-folder-mark" />
          </span>
        </div>
        <p className="fr-eyebrow"><span className="dot" />{t("firstRun.indexing.eyebrow")}</p>
        <h1 className="fr-title">{t("firstRun.indexing.title")}</h1>
        <p className="fr-lead">{t("firstRun.indexing.body")}</p>

        <div className="fr-progress" role="status">
          <div className="fr-progress-head">
            <span className="chip indexing"><Loader2 size={13} className="spin" />{statusLabel}</span>
          </div>
          <div className="fr-bar"><span className="fr-bar-fill" /></div>
        </div>

        <FirstRunStepper activeIndex={1} />

        <p className="fr-feat-label">{t("firstRun.indexing.featuresLabel")}</p>
        <div className="fr-feat-grid">
          {features.map((feat) => (
            <div className="fr-feat" key={feat.title}>
              <span className="fr-feat-icon"><feat.icon size={18} /></span>
              <strong>{feat.title}</strong>
              <span>{feat.desc}</span>
            </div>
          ))}
        </div>

        <p className="fr-cost"><Lock size={13} />{t("home.emptyHero.costBadge")}</p>
      </div>
    </div>
  );
}

function FirstRunReadyHeader({ globalHotkey, onDismiss }: { globalHotkey: string; onDismiss: () => void }) {
  const t = useT();
  return (
    <div className="fr-ready">
      <div className="fr-banner">
        <span className="fr-banner-icon"><Check size={18} /></span>
        <div className="fr-banner-text">
          <strong>{t("firstRun.banner.title")}</strong>
          <span>{t("firstRun.banner.body", { hotkey: formatHotkeyLabel(globalHotkey) })}</span>
        </div>
        <button type="button" className="btn-icon sm" aria-label={t("firstRun.dismiss")} onClick={onDismiss}>
          <X size={15} />
        </button>
      </div>
      <FirstRunStepper activeIndex={2} />
    </div>
  );
}

function FirstRunExamples({ onRunQuery }: { onRunQuery: (query: string) => void }) {
  const t = useT();
  return (
    <div className="fr-examples">
      <p className="fr-examples-label">{t("firstRun.examplesLabel")}</p>
      <div className="fr-example-row">
        {FIRST_RUN_EXAMPLE_KEYS.map(({ key, icon: Icon, tagKey }) => {
          const text = t(key);
          return (
            <button type="button" className="fr-example" key={key} onClick={() => onRunQuery(text)}>
              <Icon size={15} className="fr-example-icon" />
              <span>{text}</span>
              {tagKey ? <span className="fr-example-tag">{t(tagKey)}</span> : null}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function HomeScreen({
  query,
  setQuery,
  onSubmit,
  onAddSource,
  onOpenItem,
  onOpenLibrary,
  items,
  sources,
  jobs,
  indexingPaused,
  apiStatus,
  globalHotkey,
  firstRunActive,
  onResolveFirstRun,
  onRunQuery,
}: {
  query: string;
  setQuery: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onAddSource: () => void;
  onOpenItem: (item: Item, timestamp?: string | null) => void;
  onOpenLibrary: () => void;
  items: Item[];
  sources: Source[];
  jobs: api.JobRecord[];
  indexingPaused: boolean;
  apiStatus: ApiStatus;
  globalHotkey: string;
  firstRunActive: boolean;
  onResolveFirstRun: () => void;
  onRunQuery: (query: string) => void;
}) {
  const t = useT();
  const indexedCount = items.filter((item) => item.status === "indexed").length;
  const activeSources = sources.filter((source) => source.status === "active").length;
  const erroredSources = sources.filter((source) => source.status === "error");
  const activeJobs = jobs.filter(isActiveJob);
  const runningJobs = activeJobs.filter((job) => job.status === "running");
  const queuedJobs = activeJobs.filter((job) => job.status === "queued");
  const onlyPausedQueuedJobs = indexingPaused && runningJobs.length === 0 && queuedJobs.length > 0;
  const hasSources = sources.length > 0;
  const searchDisabled = hasSources && indexedCount === 0;
  const blockedBySourceErrors = searchDisabled && activeJobs.length === 0 && erroredSources.length > 0;
  const runtimeMinutes = Math.round(
    items.reduce((total, item) => total + durationMinutes(item.duration), 0),
  );
  const runtimeHours = Math.floor(runtimeMinutes / 60);
  const runtimeRemainder = runtimeMinutes % 60;
  const recentIndexed = [...items]
    .filter((item) => item.status === "indexed")
    .sort((left, right) => (right.indexedAtEpoch ?? 0) - (left.indexedAtEpoch ?? 0))
    .slice(0, 4);
  const [weeklyReview, setWeeklyReview] = useState<api.WeeklyReview | null>(null);
  const [showWeekly, setShowWeekly] = useState(false);
  const serverContinueItem = items
    .filter((item) => item.status === "indexed" && item.playbackPosition?.updated_at)
    .sort(
      (left, right) =>
        (right.playbackPosition?.updated_at ?? 0) - (left.playbackPosition?.updated_at ?? 0),
    )[0];
  const lastOpened = readLastOpened();
  const fallbackContinueItem =
    lastOpened
      ? items.find((item) => item.id === lastOpened.itemId && item.status === "indexed")
      : undefined;
  const fallbackTimestampSec =
    fallbackContinueItem && lastOpened?.timestamp
      ? parseTimestampSeconds(lastOpened.timestamp)
      : Number.NaN;
  const fallbackIsUseful =
    fallbackContinueItem &&
    lastOpened &&
    (!Number.isFinite(fallbackTimestampSec) ||
      !isNearEndPosition(fallbackTimestampSec, fallbackContinueItem.durationSec));
  const serverUpdatedAtMs = (serverContinueItem?.playbackPosition?.updated_at ?? 0) * 1000;
  const preferFallbackContinue =
    Boolean(fallbackIsUseful && lastOpened && (!serverContinueItem || lastOpened.at > serverUpdatedAtMs));
  const continueItem = preferFallbackContinue ? fallbackContinueItem : serverContinueItem;
  const continueTimestamp =
    continueItem?.playbackPosition?.timestamp ??
    (continueItem && lastOpened && continueItem.id === lastOpened.itemId
      ? lastOpened.timestamp
      : null);

  const statusLabel = (() => {
    if (onlyPausedQueuedJobs) {
      return t("home.status.pausedQueuedJobs", { count: queuedJobs.length });
    }
    if (activeJobs.length > 0) {
      return t("home.status.indexingJobs", { count: activeJobs.length });
    }
    if (apiStatus !== "online") {
      return coreStatusText(apiStatus, t);
    }
    if (blockedBySourceErrors) {
      return t("home.status.sourceErrors", { count: erroredSources.length });
    }
    if (searchDisabled) {
      return t("home.status.indexingFirst");
    }
    return t("home.status.indexedCount", { count: indexedCount });
  })();

  function handleSearchSubmit(event: FormEvent<HTMLFormElement>) {
    if (searchDisabled) {
      event.preventDefault();
      return;
    }

    onSubmit(event);
  }

  useEffect(() => {
    let cancelled = false;
    if (apiStatus !== "online") {
      return;
    }
    api
      .weeklyReview()
      .then((review) => {
        if (!cancelled) {
          setWeeklyReview(review.has_data ? review : null);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [apiStatus, indexedCount, activeJobs.length]);

  if (!hasSources && apiStatus === "online") {
    return <HomeEmptyState onAddSource={onAddSource} />;
  }

  const firstRunIndexing =
    firstRunActive && searchDisabled && activeJobs.length > 0 && !onlyPausedQueuedJobs;
  const firstRunReady = firstRunActive && apiStatus === "online" && indexedCount > 0;

  if (firstRunIndexing) {
    return <FirstRunIndexing statusLabel={statusLabel} globalHotkey={globalHotkey} />;
  }

  return (
    <div className="page home-page" style={{ maxWidth: 920 }}>
      {firstRunReady ? (
        <FirstRunReadyHeader globalHotkey={globalHotkey} onDismiss={onResolveFirstRun} />
      ) : null}
      <div className="home-search-stage">
        <h1>{t("home.heading")}</h1>
        <p className="muted home-summary">
          {t("home.summary", {
            count: indexedCount,
            runtime:
              runtimeHours > 0
                ? t("home.runtime.hm", { hours: runtimeHours, minutes: runtimeRemainder })
                : t("home.runtime.m", { minutes: runtimeMinutes || 0 }),
            sources: activeSources,
          })}
        </p>

        <form
          className={searchDisabled ? "search-wrap disabled" : "search-wrap"}
          onSubmit={handleSearchSubmit}
          style={{ width: "100%", maxWidth: 720, marginTop: 28 }}
        >
          <Search size={18} />
          <input
            className="search-input"
            name="query"
            disabled={searchDisabled}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={submitSearchInputOnEnter}
            placeholder={
              searchDisabled ? t("home.searchLockedPlaceholder") : t("home.searchPlaceholder")
            }
            aria-label={t("home.searchAria")}
            aria-describedby={searchDisabled ? "home-search-helper" : undefined}
          />
        </form>
        {searchDisabled ? (
          <p className="field-hint" id="home-search-helper" style={{ marginTop: 10 }}>
            {blockedBySourceErrors
              ? t("home.lockedHintSourceErrors", { count: erroredSources.length })
              : t("home.lockedHint")}
          </p>
        ) : null}

        {firstRunReady ? (
          <FirstRunExamples onRunQuery={onRunQuery} />
        ) : null}

        <div className="row gap-3 home-status-line">
          {activeJobs.length > 0 && !onlyPausedQueuedJobs ? (
            <span className="chip indexing">
              <Loader2 size={13} className="spin" />
              {statusLabel}
            </span>
          ) : (
            <span className="chip neutral">
              <span className="dot" />
              {statusLabel}
            </span>
          )}
          <span className="faint home-hotkey">{t("home.hotkeyHint", { hotkey: formatHotkeyLabel(globalHotkey) })}</span>
        </div>
      </div>

      {continueItem ? (
        <div className="home-continue-block">
          <div className="home-block-head">
            <p className="section-label">{t("home.continueWatching")}</p>
            <button className="btn btn-ghost sm" type="button" onClick={onAddSource}>
              <Plus size={14} />
              <span>{t("home.addSource")}</span>
            </button>
          </div>
          <ContinueWatchingCard
            item={continueItem}
            timestamp={continueTimestamp}
            onOpen={() => onOpenItem(continueItem, continueTimestamp)}
          />
        </div>
      ) : null}

      <div className="home-recent-block">
        <div className="home-block-head">
          <p className="section-label">{t("home.recentIndexed")}</p>
          <div className="row gap-2">
            {weeklyReview ? (
              <button
                className={showWeekly ? "btn btn-ghost sm active" : "btn btn-ghost sm"}
                type="button"
                onClick={() => setShowWeekly((value) => !value)}
              >
                <Sparkles size={14} />
                <span>{t("weekly.title")}</span>
              </button>
            ) : null}
            <button className="btn btn-ghost sm" type="button" onClick={onOpenLibrary}>
              <span>{t("home.browseLibrary")}</span>
              <ChevronRight size={14} />
            </button>
            {!continueItem ? (
              <button className="btn btn-ghost sm" type="button" onClick={onAddSource}>
                <Plus size={14} />
                <span>{t("home.addSource")}</span>
              </button>
            ) : null}
          </div>
        </div>

        {showWeekly && weeklyReview ? (
          <section className="weekly-card" aria-label={t("weekly.title")}>
            <div>
              <p className="section-label">{t("weekly.eyebrow")}</p>
              <h2>{t("weekly.title")}</h2>
              <p>
                {t("weekly.body", {
                  items: weeklyReview.indexed_items,
                  hours: formatWeeklyHours(weeklyReview.indexed_seconds),
                  watched: weeklyReview.watched_percent,
                })}
              </p>
            </div>
            <button
              type="button"
              className="btn-icon sm"
              aria-label={t("common.close")}
              onClick={() => setShowWeekly(false)}
            >
              <X size={15} />
            </button>
          </section>
        ) : null}

        {recentIndexed.length > 0 ? (
          <div className="home-recent-grid">
            {recentIndexed.map((item) => (
              <RecentIndexedCard key={item.id} item={item} onOpen={() => onOpenItem(item)} />
            ))}
          </div>
        ) : (
          <EmptyState
            title={t("library.empty.none.title")}
            body={t("library.empty.none.body")}
            actionLabel={t("library.empty.addSource")}
            onAction={onAddSource}
          />
        )}
      </div>
    </div>
  );
}

function ContinueWatchingCard({
  item,
  timestamp,
  onOpen,
}: {
  item: Item;
  timestamp: string | null;
  onOpen: () => void;
}) {
  const t = useT();
  const positionSec = item.playbackPosition?.position_sec ?? null;
  const progressPct =
    positionSec != null && item.durationSec
      ? Math.min(100, Math.max(2, (positionSec / item.durationSec) * 100))
      : null;
  const remaining =
    positionSec != null && item.durationSec
      ? formatDuration(Math.max(0, item.durationSec - positionSec))
      : null;
  const sourceLabel = item.source || t("home.continueLocal");
  return (
    <button className="cw-banner" type="button" onClick={onOpen} title={t("home.continueResume")}>
      {item.thumbnailUrl ? (
        <img className="cw-bg" src={item.thumbnailUrl} alt="" loading="lazy" />
      ) : null}
      <span className="cw-noise" aria-hidden="true" />
      <span className="cw-glow" aria-hidden="true" />
      <span className="cw-scrim" aria-hidden="true" />
      <span className="cw-play" aria-hidden="true">
        <Play size={20} fill="currentColor" />
      </span>
      <span className="cw-badge mono">
        <span className="cw-badge-dot" aria-hidden="true" />
        {sourceLabel}
      </span>
      {item.duration ? <span className="cw-dur mono">{item.duration}</span> : null}
      <span className="cw-bottom">
        <span className="cw-info">
          <strong className="cw-title clamp1">{item.title}</strong>
          <span className="cw-meta">
            {timestamp
              ? `${t("home.continueAt", { at: timestamp, total: item.duration })}${
                  remaining ? ` · ${t("home.continueRemaining", { remaining })}` : ""
                }`
              : itemKindLabel(item, t)}
          </span>
        </span>
        <span className="cw-resume">
          <Play size={13} fill="currentColor" />
          {t("home.continuePlay")}
        </span>
      </span>
      {progressPct != null ? (
        <span className="cw-bar" aria-hidden="true">
          <span style={{ width: `${progressPct}%` }} />
        </span>
      ) : null}
    </button>
  );
}

function RecentIndexedCard({ item, onOpen }: { item: Item; onOpen: () => void }) {
  const t = useT();
  return (
    <button className="card hover lib-card recent-indexed-card" type="button" onClick={onOpen}>
      <span className={`thumb ${item.thumbnailUrl ? "has-image" : item.color}`}>
        {item.thumbnailUrl ? (
          <img src={item.thumbnailUrl} alt="" loading="lazy" />
        ) : (
          <ItemModalityIcon item={item} size={20} />
        )}
        {item.contentType !== "image" && item.duration ? (
          <small className="thumb-duration mono">{item.duration}</small>
        ) : null}
      </span>
      <span className="body">
        <strong className="clamp2">{item.title}</strong>
        <span className="recent-card-meta muted">
          {item.contentType !== "video" ? <ItemModalityIcon item={item} size={13} /> : null}
          <span>
            {item.indexedAtEpoch === null
              ? t("library.itemCard.notIndexed")
              : t("library.itemCard.indexedAt", { when: item.indexedAt })}
          </span>
        </span>
        {item.visualIndexStatus === "failed" ? (
          <span className="item-warning chip warn">
            <span className="dot" />
            {t("library.itemCard.transcriptOnly")}
          </span>
        ) : null}
        {item.embeddingIndexStatus === "failed" ? (
          <span className="item-warning chip warn">
            <span className="dot" />
            {t("library.itemCard.partialIndex")}
          </span>
        ) : null}
      </span>
    </button>
  );
}
