import { useEffect, useId, useState, type CSSProperties, type FormEvent } from "react";
import { brandAssets } from "../lib/brand";
import type { TFunction } from "../lib/i18n";

// Login / register surface for Cerul Cloud — a self-contained, inline-styled
// modal ported from the design handoff (designs/CerulAuthModal.jsx). It is purely
// presentational: the auth store, OAuth, validation errors and busy state are
// owned by the caller (account-sidebar.tsx) and threaded in as props.

export type AuthMode = "signin" | "register";
type Theme = "light" | "dark";

type AuthModalProps = {
  theme: Theme;
  accent?: string;
  mode: AuthMode;
  email: string;
  password: string;
  busy: boolean;
  error: string | null;
  t: TFunction;
  onModeChange: (mode: AuthMode) => void;
  onEmailChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onGoogle: () => void;
  onGithub: () => void;
  onClose: () => void;
};

// Reads the resolved theme off <html data-theme>, staying in sync if the user
// flips themes while the modal is open.
export function useResolvedTheme(): Theme {
  const read = (): Theme =>
    document.documentElement.dataset.theme === "dark" ? "dark" : "light";
  const [theme, setTheme] = useState<Theme>(read);
  useEffect(() => {
    const root = document.documentElement;
    const update = () => setTheme(root.dataset.theme === "dark" ? "dark" : "light");
    update();
    const observer = new MutationObserver(update);
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);
  return theme;
}

function tokens(theme: Theme, accent: string) {
  const dark = theme === "dark";
  // Lift the accent on dark panels so it keeps contrast.
  const acc = dark ? `color-mix(in srgb, ${accent} 70%, #ffffff)` : accent;
  return {
    dark,
    acc,
    bg: dark ? "#080b0f" : "#e6ebf0",
    bg2: dark ? "#12171d" : "#f5f8fa",
    panel: dark ? "rgba(25,31,38,.92)" : "#ffffff",
    ink: dark ? "#e9eef3" : "#181f27",
    muted: dark ? "#94a1ae" : "#69727f",
    faint: dark ? "#6b7682" : "#9aa4b0",
    line: dark ? "#2b343d" : "#e6e9ee",
    field: dark ? "rgba(13,18,23,.66)" : "#fafbfc",
    fieldLine: dark ? "#2c353e" : "#dce1e8",
    accentInk: dark ? "#0c1116" : "#ffffff",
    panelEdge: dark ? "rgba(255,255,255,.12)" : "rgba(255,255,255,.9)",
    scrim: dark ? "rgba(4,7,10,.6)" : "rgba(226,232,238,.5)",
    ring: `color-mix(in srgb, ${accent} ${dark ? "45" : "30"}%, transparent)`,
    btnGrad: `linear-gradient(180deg, color-mix(in srgb, ${acc} 92%, #fff), color-mix(in srgb, ${acc} 86%, #000))`,
    btnShadow: `0 8px 18px -8px color-mix(in srgb, ${acc} 70%, transparent), inset 0 1px 0 rgba(255,255,255,.28)`,
    tabBg: `color-mix(in srgb, ${accent} 7%, ${dark ? "rgba(13,18,23,.66)" : "#fafbfc"})`,
    shadow: dark
      ? "0 1px 0 rgba(255,255,255,.06) inset, 0 46px 96px -30px rgba(0,0,0,.85), 0 8px 26px -10px rgba(0,0,0,.6)"
      : "0 1px 0 rgba(255,255,255,.8) inset, 0 32px 72px -26px rgba(22,32,48,.4), 0 4px 14px -6px rgba(22,32,48,.16)",
    errorBg: dark ? "rgba(220,80,72,.12)" : "#fdeceb",
    errorLine: dark ? "rgba(220,80,72,.32)" : "#f3c7c2",
    errorInk: dark ? "#f1a8a1" : "#b3261e",
    fieldHover: dark ? "color-mix(in srgb, #fff 3%, rgba(13,18,23,.66))" : "color-mix(in srgb, #000 3%, #fafbfc)",
    fieldHoverLine: dark ? "color-mix(in srgb, #fff 18%, transparent)" : "color-mix(in srgb, #000 18%, transparent)",
  };
}

/* ---------- icons ---------- */
const IconClose = () => (
  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M6 6l12 12M18 6L6 18" /></svg>
);
const IconArrow = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12h14M13 6l6 6-6 6" /></svg>
);
const IconEye = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="M1 12s4-7 11-7 11 7 11 7-4 7-11 7-11-7-11-7z" /><circle cx="12" cy="12" r="3" /></svg>
);
const IconEyeOff = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24" /><line x1="1" y1="1" x2="23" y2="23" /></svg>
);
const IconShield = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" /></svg>
);
const IconAlert = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10" /><path d="M12 8v4M12 16h.01" /></svg>
);
const IconGoogle = () => (
  <svg width="16" height="16" viewBox="0 0 48 48" aria-hidden="true"><path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" /><path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" /><path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" /><path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" /></svg>
);
const IconGithub = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.37.5 0 5.87 0 12.5c0 5.3 3.44 9.8 8.21 11.39.6.11.82-.26.82-.58 0-.29-.01-1.04-.02-2.05-3.34.73-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.09-.75.08-.73.08-.73 1.21.09 1.84 1.24 1.84 1.24 1.07 1.84 2.81 1.31 3.5 1 .11-.78.42-1.31.76-1.61-2.67-.3-5.47-1.34-5.47-5.95 0-1.31.47-2.39 1.24-3.23-.12-.3-.54-1.52.12-3.18 0 0 1.01-.32 3.3 1.23a11.5 11.5 0 0 1 6 0c2.29-1.55 3.3-1.23 3.3-1.23.66 1.66.24 2.88.12 3.18.77.84 1.24 1.92 1.24 3.23 0 4.62-2.81 5.64-5.49 5.94.43.37.81 1.1.81 2.22 0 1.6-.01 2.89-.01 3.29 0 .32.22.7.83.58A12.01 12.01 0 0 0 24 12.5C24 5.87 18.63.5 12 .5z" /></svg>
);

export function AuthModal({
  theme,
  accent = "#3E6B9D",
  mode,
  email,
  password,
  busy,
  error,
  t,
  onModeChange,
  onEmailChange,
  onPasswordChange,
  onSubmit,
  onGoogle,
  onGithub,
  onClose,
}: AuthModalProps) {
  const [showPw, setShowPw] = useState(false);
  const scope = "cl-" + useId().replace(/:/g, "");
  const tk = tokens(theme, accent);
  const isLogin = mode === "signin";

  const rootStyle: CSSProperties = {
    position: "fixed",
    inset: 0,
    zIndex: 1000,
    display: "grid",
    placeItems: "center",
    padding: 40,
    overflow: "auto",
    fontFamily: '-apple-system, system-ui, "PingFang SC", "Segoe UI", sans-serif',
    WebkitFontSmoothing: "antialiased",
    ["--cl-accent" as string]: tk.acc,
    ["--cl-ring" as string]: tk.ring,
    ["--cl-field" as string]: tk.field,
    ["--cl-field-line" as string]: tk.fieldLine,
    ["--cl-field-hover" as string]: tk.fieldHover,
    ["--cl-field-hover-line" as string]: tk.fieldHoverLine,
    ["--cl-ink" as string]: tk.ink,
    ["--cl-faint" as string]: tk.faint,
  };

  const fieldStyle: CSSProperties = {
    width: "100%",
    borderRadius: 11,
    font: "400 13.5px system-ui",
    background: tk.field,
    border: `1px solid ${tk.fieldLine}`,
    color: tk.ink,
    padding: "12px 14px",
    transition: ".16s",
  };

  const socialStyle: CSSProperties = {
    flex: 1,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    background: tk.field,
    border: `1px solid ${tk.fieldLine}`,
    borderRadius: 11,
    padding: 11,
    font: "600 12.5px system-ui",
    color: tk.ink,
    cursor: busy ? "default" : "pointer",
    opacity: busy ? 0.6 : 1,
    transition: ".16s",
  };

  return (
    <div className={scope} style={rootStyle}>
      <style>{`
        .${scope} *{box-sizing:border-box}
        .${scope} input{outline:none}
        .${scope} input::placeholder{color:var(--cl-faint);opacity:1}
        .${scope} input:focus{border-color:var(--cl-accent);box-shadow:0 0 0 3px var(--cl-ring)}
        .${scope} .cl-social:not(:disabled):hover{border-color:var(--cl-field-hover-line);background:var(--cl-field-hover)}
        .${scope} .cl-icon-btn:hover{color:var(--cl-ink)}
        .${scope} .cl-btn:not(:disabled):hover{transform:translateY(-1px)}
        .${scope} .cl-btn:not(:disabled):active{transform:translateY(0)}
        @keyframes ${scope}-in{from{opacity:0;transform:translateY(14px) scale(.97)}to{opacity:1;transform:none}}
      `}</style>

      {/* backdrop: frost the actual app behind the modal (sense of place when
          there's a library behind) AND lay an accent aurora over it so it's
          never blank even over a sparse view; the vignette focuses the card. */}
      <div
        onClick={onClose}
        style={{
          position: "absolute",
          inset: 0,
          background: tk.dark
            ? "radial-gradient(130% 100% at 50% -10%, rgba(18,23,29,.5), rgba(8,11,15,.72))"
            : "radial-gradient(130% 100% at 50% -10%, rgba(244,247,250,.42), rgba(221,229,237,.6))",
          backdropFilter: "blur(26px) saturate(1.2)",
          WebkitBackdropFilter: "blur(26px) saturate(1.2)",
        }}
      />
      <div style={{ position: "absolute", width: 680, height: 680, left: -180, top: -230, borderRadius: "50%", background: `radial-gradient(circle, color-mix(in srgb, ${accent} 42%, transparent), transparent 66%)`, filter: "blur(34px)", pointerEvents: "none" }} />
      <div style={{ position: "absolute", width: 600, height: 600, right: -200, bottom: -240, borderRadius: "50%", background: `radial-gradient(circle, color-mix(in srgb, ${accent} 30%, transparent), transparent 64%)`, filter: "blur(36px)", pointerEvents: "none" }} />
      <div style={{ position: "absolute", width: 460, height: 460, left: "50%", top: "16%", transform: "translateX(-50%)", borderRadius: "50%", background: `radial-gradient(circle, color-mix(in srgb, ${accent} 18%, transparent), transparent 70%)`, filter: "blur(40px)", pointerEvents: "none" }} />
      <div style={{ position: "absolute", inset: 0, boxShadow: `inset 0 0 240px 50px ${tk.scrim}`, pointerEvents: "none" }} />

      {/* panel */}
      <form
        onSubmit={onSubmit}
        role="dialog"
        aria-modal="true"
        style={{
          position: "relative",
          width: 404,
          maxWidth: "100%",
          background: tk.panel,
          border: `1px solid ${tk.line}`,
          borderRadius: 22,
          boxShadow: tk.shadow,
          backdropFilter: "blur(22px) saturate(1.1)",
          WebkitBackdropFilter: "blur(22px) saturate(1.1)",
          padding: "34px 32px 26px",
          animation: `${scope}-in .5s cubic-bezier(.2,.85,.25,1)`,
        }}
      >
        <div style={{ position: "absolute", top: 0, left: 26, right: 26, height: 1, background: `linear-gradient(90deg, transparent, ${tk.panelEdge}, transparent)` }} />

        <div className="cl-icon-btn" onClick={onClose} role="button" aria-label={t("common.close")} style={{ position: "absolute", top: 16, right: 16, width: 30, height: 30, display: "grid", placeItems: "center", borderRadius: 9, color: tk.faint, cursor: "pointer", transition: ".16s" }}>
          <IconClose />
        </div>

        {/* brand + title */}
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center" }}>
          <img
            src={tk.dark ? brandAssets.markDark : brandAssets.markLight}
            alt=""
            draggable={false}
            style={{ width: 48, height: 48, objectFit: "contain" }}
          />
          <div style={{ font: "700 21px/1.25 system-ui", letterSpacing: "-.02em", color: tk.ink, marginTop: 14 }}>
            {isLogin ? t("settings.account.welcomeBack") : t("settings.account.createTitle")}
          </div>
          <div style={{ font: "400 13px/1.55 system-ui", color: tk.muted, marginTop: 7, maxWidth: 280 }}>
            {t("settings.account.subtitle")}
          </div>
        </div>

        {/* segmented tabs */}
        <div style={{ position: "relative", display: "flex", marginTop: 22, background: tk.tabBg, border: `1px solid ${tk.line}`, borderRadius: 12, padding: 4 }}>
          <div style={{ position: "absolute", top: 4, left: isLogin ? 4 : "50%", width: "calc(50% - 4px)", height: "calc(100% - 8px)", background: tk.panel, borderRadius: 9, boxShadow: "0 1px 3px rgba(20,30,45,.12), 0 0 0 1px rgba(20,30,45,.03)", transition: "left .28s cubic-bezier(.3,.8,.3,1)" }} />
          <button type="button" onClick={() => onModeChange("signin")} aria-pressed={isLogin} style={{ position: "relative", zIndex: 1, flex: 1, textAlign: "center", font: "600 13px system-ui", color: isLogin ? tk.ink : tk.muted, padding: "8px 0", borderRadius: 9, cursor: "pointer", border: 0, background: "transparent" }}>{t("settings.account.signIn")}</button>
          <button type="button" onClick={() => onModeChange("register")} aria-pressed={!isLogin} style={{ position: "relative", zIndex: 1, flex: 1, textAlign: "center", font: "600 13px system-ui", color: !isLogin ? tk.ink : tk.muted, padding: "8px 0", borderRadius: 9, cursor: "pointer", border: 0, background: "transparent" }}>{t("settings.account.createAccount")}</button>
        </div>

        {/* fields */}
        <div style={{ display: "flex", flexDirection: "column", gap: 14, marginTop: 20 }}>
          <div>
            <div style={{ font: "600 11.5px/1 system-ui", color: tk.muted, marginBottom: 7 }}>{t("settings.account.email")}</div>
            <input
              type="email"
              autoComplete="email"
              autoFocus
              placeholder="you@example.com"
              value={email}
              disabled={busy}
              onChange={(e) => onEmailChange(e.target.value)}
              style={fieldStyle}
            />
          </div>
          <div>
            <div style={{ font: "600 11.5px/1 system-ui", color: tk.muted, marginBottom: 7 }}>{t("settings.account.password")}</div>
            <div style={{ position: "relative" }}>
              <input
                type={showPw ? "text" : "password"}
                autoComplete={isLogin ? "current-password" : "new-password"}
                placeholder={t("settings.account.passwordPlaceholder")}
                value={password}
                disabled={busy}
                onChange={(e) => onPasswordChange(e.target.value)}
                style={{ ...fieldStyle, padding: "12px 42px 12px 14px" }}
              />
              <div className="cl-icon-btn" onClick={() => setShowPw((s) => !s)} role="button" aria-label={t("settings.account.togglePassword")} style={{ position: "absolute", right: 8, top: "50%", transform: "translateY(-50%)", width: 30, height: 30, display: "grid", placeItems: "center", borderRadius: 8, color: tk.faint, cursor: "pointer", transition: ".16s" }}>
                {showPw ? <IconEyeOff /> : <IconEye />}
              </div>
            </div>
            {!isLogin && (
              <div style={{ font: "400 11.5px/1.4 system-ui", color: tk.faint, marginTop: 7 }}>{t("settings.account.passwordHint")}</div>
            )}
          </div>
        </div>

        {error && (
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 14, padding: "10px 12px", borderRadius: 10, background: tk.errorBg, border: `1px solid ${tk.errorLine}`, color: tk.errorInk, font: "500 12.5px/1.45 system-ui" }}>
            <span style={{ display: "inline-flex", flex: "none" }}><IconAlert /></span>
            <span>{error}</span>
          </div>
        )}

        {/* primary */}
        <button type="submit" className="cl-btn" disabled={busy} style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 8, width: "100%", marginTop: 18, background: tk.btnGrad, color: tk.accentInk, font: "650 14px system-ui", border: "none", borderRadius: 12, padding: 13, cursor: busy ? "default" : "pointer", opacity: busy ? 0.75 : 1, boxShadow: tk.btnShadow, transition: ".18s" }}>
          <span>{busy ? t("settings.account.working") : isLogin ? t("settings.account.signIn") : t("settings.account.createAccount")}</span>
          {!busy && <IconArrow />}
        </button>

        {/* divider */}
        <div style={{ display: "flex", alignItems: "center", gap: 12, margin: "18px 0" }}>
          <div style={{ flex: 1, height: 1, background: tk.line }} />
          <span style={{ font: "500 11px system-ui", color: tk.faint }}>{t("settings.account.orContinue")}</span>
          <div style={{ flex: 1, height: 1, background: tk.line }} />
        </div>

        {/* social */}
        <div style={{ display: "flex", gap: 10 }}>
          <button type="button" className="cl-social" disabled={busy} onClick={onGoogle} style={socialStyle}>
            <IconGoogle /> Google
          </button>
          <button type="button" className="cl-social" disabled={busy} onClick={onGithub} style={socialStyle}>
            <IconGithub /> GitHub
          </button>
        </div>

        {/* reassurance */}
        <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 7, marginTop: 22, paddingTop: 16, borderTop: `1px solid ${tk.line}`, font: "400 11.5px/1.4 system-ui", color: tk.faint, textAlign: "center" }}>
          <span style={{ color: "#4fae7e", display: "inline-flex", flex: "none" }}><IconShield /></span>
          {t("settings.account.reassure")}
        </div>
      </form>
    </div>
  );
}
