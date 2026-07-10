import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  Database,
  Library,
  ListChecks,
  Search,
  Settings,
  Star,
  User,
  type LucideIcon,
} from "lucide-react";
import { useT } from "../lib/i18n";
import { useEscapeToClose } from "../lib/use-dismissable";
import { useAuthStore } from "../lib/cloud/authStore";
import { BrandMark } from "./brand";
import type * as api from "../lib/api";

// 舰桥（Bridge）— 顶部悬浮导航，2026-07-10 主题定稿（cerul-brand I_应用主题）。
// 恒暗胶囊：mark → 页签 → 搜索（呼吸态）→ 任务 → 头像菜单。
// 纯展示层重组：导航/任务/设置/账户/主题全部复用 App 既有状态与动作。

export type BridgeView = "home" | "library" | "moments" | "sources" | "settings";

type BridgeProps = {
  activeView: string;
  onboardingActive: boolean;
  onNavigate: (view: BridgeView) => void;
  onOpenJobs: () => void;
  jobsCount: number;
  coreLevel: string;
  coreLabel: string;
  /* search (呼吸态) — hidden on home, whose hero stage is the search surface */
  searchVisible: boolean;
  query: string;
  onRunQuery: (q: string) => void;
  rankingPreference: api.SearchRankingPreference;
  onRankingPreferenceChange: (v: api.SearchRankingPreference) => void;
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
    jobsCount,
    coreLevel,
    coreLabel,
    searchVisible,
    query,
    onRunQuery,
    rankingPreference,
    onRankingPreferenceChange,
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
  useEffect(() => {
    setValue(query);
  }, [query]);
  const tall = searchVisible && focused;

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = value.trim();
    if (!trimmed) return;
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
      <div className={tall ? "bridge is-tall" : "bridge"}>
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
              placeholder={t("home.searchPlaceholder")}
              aria-label={t("home.searchAria")}
              disabled={onboardingActive}
              onChange={(event) => setValue(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key !== "Escape") return;
                event.preventDefault();
                event.stopPropagation();
                setFocused(false);
                inputRef.current?.blur();
              }}
            />
            <kbd className="bridge-kbd" aria-hidden="true">
              {focused ? "esc" : "⌥Space"}
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
                    onRankingPreferenceChange(preference);
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

        <button
          className="bridge-tab bridge-jobs"
          type="button"
          disabled={onboardingActive}
          onClick={onOpenJobs}
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
            <span
              className="bridge-core-dot"
              data-level={coreLevel === "grace" ? "ok" : coreLevel}
              aria-hidden="true"
            />
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
                <span
                  className="bridge-core-dot static"
                  data-level={coreLevel === "grace" ? "ok" : coreLevel}
                  aria-hidden="true"
                />
                {coreLabel}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
