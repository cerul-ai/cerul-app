// Lightweight i18n runtime for the Cerul desktop app.
//
// The string catalog lives in ./i18n-catalog.ts (generated from the design map).
// Language is UI-only state: it is the source of truth in localStorage, applied
// to <html data-lang>, and defaults to Simplified Chinese (zh) to match the
// Cerul design mock. It works fully offline and is independent of the backend
// `theme` setting (which stays driven by Cerul Core settings).

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type { ReactNode } from "react";
import { catalogs, en } from "./i18n-catalog";
import type { Lang } from "./i18n-catalog";

export type { Lang } from "./i18n-catalog";

const STORAGE_KEY = "cerul.lang.v1";
const DEFAULT_LANG: Lang = "zh";
const SUPPORTED: Lang[] = ["zh", "en"];

export type TVars = Record<string, string | number>;

function interpolate(template: string, vars?: TVars): string {
  if (!vars) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}

// Pure lookup with English fallback, then the raw key so missing strings are
// visible rather than silently blank.
/** BCP-47 tag for the app language — for number/date formatting outside of
 * React context. Falls back to the stored preference. */
export function appLocaleTag(): string {
  const lang = document.documentElement.getAttribute("lang") ?? readStoredLang();
  return lang === "zh" ? "zh-CN" : "en-US";
}

export function translate(lang: Lang, key: string, vars?: TVars): string {
  const table = catalogs[lang] ?? en;
  const template = table[key] ?? en[key] ?? key;
  return interpolate(template, vars);
}

export type TFunction = (key: string, vars?: TVars) => string;

function readStoredLang(): Lang {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw && SUPPORTED.includes(raw as Lang)) {
      return raw as Lang;
    }
  } catch {
    // Ignore storage failures — fall back to the default language.
  }
  return DEFAULT_LANG;
}

type LangContextValue = {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: TFunction;
};

const LangContext = createContext<LangContextValue | null>(null);

export function LangProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(() => readStoredLang());

  useEffect(() => {
    const root = document.documentElement;
    root.setAttribute("lang", lang);
    root.dataset.lang = lang;
  }, [lang]);

  const setLang = useCallback((next: Lang) => {
    if (!SUPPORTED.includes(next)) {
      return;
    }
    setLangState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Language preference is a convenience; ignore storage failures.
    }
  }, []);

  const value = useMemo<LangContextValue>(
    () => ({
      lang,
      setLang,
      t: (key: string, vars?: TVars) => translate(lang, key, vars),
    }),
    [lang, setLang],
  );

  return <LangContext.Provider value={value}>{children}</LangContext.Provider>;
}

function useLangContext(): LangContextValue {
  const ctx = useContext(LangContext);
  if (ctx) {
    return ctx;
  }
  // Defensive fallback so components used outside a provider (e.g. isolated
  // tests) still render with the default language instead of throwing.
  return {
    lang: DEFAULT_LANG,
    setLang: () => undefined,
    t: (key: string, vars?: TVars) => translate(DEFAULT_LANG, key, vars),
  };
}

// Primary hook: returns the translate function `t`.
export function useT(): TFunction {
  return useLangContext().t;
}

// Returns { lang, setLang } for the Settings language switcher.
export function useLang(): { lang: Lang; setLang: (lang: Lang) => void } {
  const { lang, setLang } = useLangContext();
  return { lang, setLang };
}

// Full context accessor when a component needs both t and lang controls.
export function useI18n(): LangContextValue {
  return useLangContext();
}
