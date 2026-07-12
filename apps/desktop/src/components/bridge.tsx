import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  Database,
  Library,
  Link2,
  ListChecks,
  Pause,
  Play,
  Search,
  Settings,
  Star,
  User,
  type LucideIcon,
} from "lucide-react";
import { useT } from "../lib/i18n";
import { useClickOutside, useEscapeToClose } from "../lib/use-dismissable";
import { useAuthStore } from "../lib/cloud/authStore";
import { BrandMark } from "./brand";
import type * as api from "../lib/api";
import type { Item } from "../lib/types";
import { isActiveJob } from "../lib/items";
import {
  jobBadgeStatus,
  jobDisplayStatus,
  jobItemTitle,
  jobStageMessage,
  jobStepProgressPercent,
  jobTypeLabel,
} from "../lib/jobs";

// 舰桥（Bridge）— 顶部悬浮导航，2026-07-10 主题定稿（cerul-brand I_应用主题）。
// 恒暗胶囊：mark → 页签 → 搜索（呼吸态）→ 任务 → 头像菜单。
// 纯展示层重组：导航/任务/设置/账户/主题全部复用 App 既有状态与动作。

export type BridgeView = "home" | "library" | "moments" | "sources" | "shares" | "settings";

type BridgeProps = {
  activeView: string;
  onboardingActive: boolean;
  onNavigate: (view: BridgeView) => void;
  onOpenJobs: () => void;
  jobs: api.JobRecord[];
  jobsSummary: api.JobStatusSummary | null;
  items: Item[];
  indexingPaused: boolean;
  onTogglePause: () => void;
  jobsCount: number;
  coreLabel: string;
  /* search (呼吸态) — hidden on home, whose hero stage is the search surface */
  searchVisible: boolean;
  query: string;
  onRunQuery: (q: string) => void;
  rankingPreference: api.SearchRankingPreference;
  onRankingPreferenceChange: (v: api.SearchRankingPreference, draftQuery: string) => void;
  hotkeyLabel: string;
  /* theme cycles through the same persisted setting used by Settings */
  themePreference: string;
  themeLabel: string;
  onCycleTheme: () => void;
  /* minimized local-model download pill */
  downloadPill?: { label: string; onReopen: () => void } | null;
};

const NAV_ITEMS: { id: BridgeView; labelKey: string; icon: LucideIcon }[] = [
  { id: "home", labelKey: "nav.home", icon: Search },
  { id: "library", labelKey: "nav.library", icon: Library },
  { id: "moments", labelKey: "nav.moments", icon: Star },
  { id: "sources", labelKey: "nav.sources", icon: Database },
];

const RANKING_VALUES: api.SearchRankingPreference[] = [
  "smart",
  "video",
  "image",
  "document",
  "audio",
];

export function Bridge(props: BridgeProps) {
  const t = useT();
  const {
    activeView,
    onboardingActive,
    onNavigate,
    onOpenJobs,
    jobs,
    jobsSummary,
    items,
    indexingPaused,
    onTogglePause,
    jobsCount,
    coreLabel,
    searchVisible,
    query,
    onRunQuery,
    rankingPreference,
    onRankingPreferenceChange,
    hotkeyLabel,
    themeLabel,
    onCycleTheme,
    downloadPill,
  } = props;

  /* ---- search 呼吸态 ---- */
  const [value, setValue] = useState(query);
  const [focused, setFocused] = useState(false);
  const searchRef = useRef<HTMLFormElement | null>(null);
  const scopeRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const settingsMode = activeView === "settings";
  useEffect(() => {
    setValue(settingsMode ? "" : query);
  }, [query, settingsMode]);
  const tall = searchVisible && focused && !settingsMode;

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = value.trim();
    if (!trimmed) return;
    if (settingsMode) {
      window.dispatchEvent(new CustomEvent("cerul:settings-command", { detail: trimmed }));
      return;
    }
    onRunQuery(trimmed);
    inputRef.current?.blur();
  }
  function onSearchBlur(event: React.FocusEvent<HTMLFormElement>) {
    const nextTarget = event.relatedTarget as Node | null;
    // Scope chips are visually the bridge's second row but intentionally come
    // next in DOM order so keyboard users can reach them directly from input.
    if (
      nextTarget &&
      (searchRef.current?.contains(nextTarget) || scopeRef.current?.contains(nextTarget))
    ) {
      return;
    }
    setFocused(false);
  }
  function onScopeBlur(event: React.FocusEvent<HTMLDivElement>) {
    const nextTarget = event.relatedTarget as Node | null;
    if (
      nextTarget &&
      (searchRef.current?.contains(nextTarget) || scopeRef.current?.contains(nextTarget))
    ) {
      return;
    }
    setFocused(false);
  }
  function onSearchEscape(event: React.KeyboardEvent<HTMLElement>) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    setFocused(false);
    inputRef.current?.blur();
  }

  /* ---- avatar menu ---- */
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  useEscapeToClose(() => setMenuOpen(false), menuOpen);
  useEffect(() => {
    if (!menuOpen) return;
    const onPointerDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("mousedown", onPointerDown);
    return () => window.removeEventListener("mousedown", onPointerDown);
  }, [menuOpen]);

  /* ---- E1 task glance ---- */
  const [jobsOpen, setJobsOpen] = useState(false);
  const jobsRef = useRef<HTMLDivElement | null>(null);
  useEscapeToClose(() => setJobsOpen(false), jobsOpen);
  useClickOutside(jobsRef, () => setJobsOpen(false), jobsOpen);
  // The bridge outlives page navigation, so an open glance would float over
  // whatever screen comes next — collapse it whenever the route changes.
  useEffect(() => {
    const close = () => setJobsOpen(false);
    window.addEventListener("hashchange", close);
    return () => window.removeEventListener("hashchange", close);
  }, []);
  const recentJobs = [...jobs]
    .sort((left, right) => {
      const activeDelta = Number(isActiveJob(right)) - Number(isActiveJob(left));
      if (activeDelta !== 0) return activeDelta;
      return (right.finished_at ?? right.started_at ?? 0) - (left.finished_at ?? left.started_at ?? 0);
    })
    .slice(0, 3);
  const queuedCount = jobsSummary?.queued_jobs ?? jobs.filter((job) => job.status === "queued").length;

  const status = useAuthStore((state) => state.status);
  const user = useAuthStore((state) => state.user);
  const signedIn = status === "signedIn" && !!user;
  const accountLabel = signedIn && user ? user.email : t("settings.account.signIn");

  const rankingLabel: Record<api.SearchRankingPreference, string> = {
    smart: t("results.preference.smart"),
    video: t("results.preference.video"),
    image: t("results.preference.image"),
    document: t("results.preference.document"),
    audio: t("results.preference.audio"),
  };

  return (
    <div className="bridge-wrap">
      <div className={`${tall ? "bridge is-tall" : "bridge"}${settingsMode ? " settings-mode" : ""}`}>
        <button
          className="bridge-brand"
          type="button"
          disabled={onboardingActive}
          onClick={() => onNavigate("home")}
          aria-label={t("shell.openHome")}
        >
          {/* 舰桥恒暗，无论主题都用 paper 白 glyph（图标自适应规则：暗底=纸白） */}
          <BrandMark variant="white" />
        </button>

        <nav className="bridge-nav" aria-label={t("nav.home")}>
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            const active = item.id === activeView;
            return (
              <button
                className={active ? "bridge-tab active" : "bridge-tab"}
                key={item.id}
                type="button"
                disabled={onboardingActive}
                onClick={() => onNavigate(item.id)}
                title={t(item.labelKey)}
              >
                <Icon size={15} aria-hidden="true" />
                <span className="bridge-tab-label">{t(item.labelKey)}</span>
              </button>
            );
          })}
        </nav>

        {searchVisible ? (
          <form
            ref={searchRef}
            className="bridge-search"
            role="search"
            onSubmit={submit}
            onFocus={() => setFocused(true)}
            onBlur={onSearchBlur}
          >
            <Search size={14} className="bridge-search-icon" aria-hidden="true" />
            <input
              ref={inputRef}
              type="text"
              value={value}
              placeholder={settingsMode ? t("settings.command.bridgePlaceholder") : t("home.searchPlaceholder")}
              aria-label={t("home.searchAria")}
              disabled={onboardingActive}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                setValue(nextValue);
                if (settingsMode) window.dispatchEvent(new CustomEvent("cerul:settings-command", { detail: nextValue }));
              }}
              onKeyDown={onSearchEscape}
            />
            <kbd className="bridge-kbd" aria-hidden="true">
              {focused ? "esc" : hotkeyLabel}
            </kbd>
          </form>
        ) : (
          <div className="bridge-spacer" aria-hidden="true" />
        )}

        {tall ? (
          <div
            ref={scopeRef}
            className="bridge-scope"
            role="radiogroup"
            aria-label={t("results.preference.label")}
            onBlur={onScopeBlur}
            onKeyDown={onSearchEscape}
          >
            <span className="bridge-scope-label mono">{t("results.preference.label")}</span>
            {RANKING_VALUES.map((preference) => (
              <button
                key={preference}
                type="button"
                role="radio"
                aria-checked={rankingPreference === preference}
                className={
                  rankingPreference === preference ? "bridge-scope-chip active" : "bridge-scope-chip"
                }
                // Keep input focus for mouse users so the row does not collapse;
                // click remains the single activation path for mouse and keyboard.
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  if (rankingPreference !== preference) {
                    onRankingPreferenceChange(preference, value);
                  }
                }}
              >
                {rankingLabel[preference]}
              </button>
            ))}
            <span className="bridge-scope-hint mono" aria-hidden="true">
              ↵ {t("home.searchSubmit")}
            </span>
          </div>
        ) : null}

        <div className="bridge-jobs-shell" ref={jobsRef}>
          <button
            className={jobsOpen ? "bridge-tab bridge-jobs active" : "bridge-tab bridge-jobs"}
            type="button"
            disabled={onboardingActive}
            aria-haspopup="dialog"
            aria-expanded={jobsOpen}
            onClick={() => {
              setMenuOpen(false);
              setJobsOpen((open) => !open);
            }}
            title={t("nav.jobs")}
          >
            <span className="bridge-jobs-icon">
              <ListChecks size={15} aria-hidden="true" />
              {jobsCount > 0 ? (
                <span className="badge-count" aria-hidden="true">
                  {jobsCount > 9 ? "9+" : jobsCount}
                </span>
              ) : null}
            </span>
            <span className="bridge-tab-label">{t("nav.jobs")}</span>
          </button>

          {jobsOpen ? (
            <section className="bridge-jobs-popover" role="dialog" aria-label={t("nav.jobs")}>
              <header className="bridge-jobs-popover-head">
                <span>
                  <strong>{t("nav.jobs")}</strong>
                  {/* Queued count, not the all-time job total — the footer and
                      the library banner count waiting work, and a third number
                      here read like a bug. */}
                  <small className="mono">{t("jobs.popover.queue", { count: queuedCount })} · {t("jobs.localProcessing")}</small>
                </span>
                <button type="button" onClick={onTogglePause}>
                  {indexingPaused ? <Play size={12} /> : <Pause size={12} />}
                  {indexingPaused ? t("jobs.resume") : t("jobs.pause")}
                </button>
              </header>
              <div className="bridge-jobs-popover-list">
                {recentJobs.length > 0 ? recentJobs.map((job) => {
                  const tone = jobBadgeStatus(job.status);
                  const progress = jobStepProgressPercent(job);
                  return (
                    <article className="bridge-job-glance" key={job.id} data-tone={tone}>
                      <div>
                        <strong className="clamp1">{jobItemTitle(job, items, t)}</strong>
                        <span className="clamp1">{jobTypeLabel(job.job_type, t)} · {jobStageMessage(job, t)}</span>
                      </div>
                      <em>{jobDisplayStatus(job, t)}</em>
                      {job.status === "running" ? (
                        <span className="bridge-job-progress"><i style={{ width: `${progress}%` }} /></span>
                      ) : null}
                    </article>
                  );
                }) : <p className="bridge-jobs-empty">{t("jobs.emptyBody")}</p>}
              </div>
              <footer className="bridge-jobs-popover-foot">
                <span>{t("jobs.popover.waiting", { count: queuedCount })}</span>
                <button type="button" onClick={() => { setJobsOpen(false); onOpenJobs(); }}>
                  {t("jobs.popover.viewAll")} <span aria-hidden="true">→</span>
                </button>
              </footer>
            </section>
          ) : null}
        </div>

        {downloadPill ? (
          <button
            type="button"
            className="bridge-dl-pill"
            onClick={downloadPill.onReopen}
            title={downloadPill.label}
          >
            <span className="ring" aria-hidden="true" />
          </button>
        ) : null}

        <div className="bridge-account" ref={menuRef}>
          <button
            className="bridge-avatar"
            type="button"
            disabled={onboardingActive}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
            title={accountLabel}
          >
            {signedIn && user ? (
              <span aria-hidden="true">{user.email.charAt(0).toUpperCase()}</span>
            ) : (
              <User size={14} aria-hidden="true" />
            )}
          </button>

          {menuOpen ? (
            <div className="bridge-menu" role="menu" aria-label={accountLabel}>
              <button
                className="bridge-menu-head"
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  window.dispatchEvent(new Event("cerul:open-account"));
                }}
              >
                <span className="bridge-menu-avatar" aria-hidden="true">
                  {signedIn && user ? user.email.charAt(0).toUpperCase() : <User size={14} />}
                </span>
                <span className="bridge-menu-id">
                  <b>{signedIn && user ? user.email : t("settings.account.signIn")}</b>
                  <span>{t("settings.section.account")}</span>
                </span>
              </button>
              <button
                className="bridge-menu-row"
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onNavigate("settings");
                }}
              >
                <Settings size={14} aria-hidden="true" />
                {t("nav.settings")}
              </button>
              <button
                className="bridge-menu-row"
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onNavigate("shares");
                }}
              >
                <Link2 size={14} aria-hidden="true" />
                {t("nav.shares")}
              </button>
              <button
                className="bridge-menu-row"
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onOpenJobs();
                }}
              >
                <ListChecks size={14} aria-hidden="true" />
                {t("nav.jobs")}
                {jobsCount > 0 ? <em>{jobsCount}</em> : null}
              </button>
              <button className="bridge-menu-row" type="button" role="menuitem" onClick={onCycleTheme}>
                <span className="bridge-menu-swatch" aria-hidden="true" />
                {t("settings.general.theme")}
                <em>{themeLabel}</em>
              </button>
              <div className="bridge-menu-foot mono">
                {coreLabel}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
