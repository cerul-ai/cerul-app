import { useEffect, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import {
  AlertCircle,
  CheckCircle2,
  Github,
  LogIn,
  LogOut,
  Mail,
  ShieldCheck,
  User,
  UserPlus,
} from "lucide-react";
import { useT, type TFunction } from "../lib/i18n";
import { useEscapeToClose } from "../lib/use-dismissable";
import { InlineNotice } from "./leaf";
import { BrandMark } from "./brand";
import { useAuthStore } from "../lib/cloud/authStore";
import { cloudClient } from "../lib/cloud/client";
import { CloudApiError } from "../lib/cloud/types";
import { startDesktopOAuth } from "../lib/desktopHost";

type AuthMode = "signin" | "register";

// Google's wordmark glyph (lucide has no brand mark for it). Matches the
// prototype's OAuth row.
function GoogleMark() {
  return (
    <svg width={15} height={15} viewBox="0 0 48 48" aria-hidden="true">
      <path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" />
      <path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" />
      <path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" />
      <path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" />
    </svg>
  );
}

function friendlyError(error: unknown, t: TFunction): string {
  if (error instanceof CloudApiError) {
    switch (error.code) {
      case "invalid_credentials":
        return t("settings.account.error.invalidCredentials");
      case "email_already_registered":
        return t("settings.account.error.emailTaken");
      case "weak_password":
        return t("settings.account.error.weakPassword");
      case "invalid_email":
        return t("settings.account.error.invalidEmail");
      case "invalid_verification_code":
        return t("settings.account.error.invalidCode");
      case "verification_code_expired":
        return t("settings.account.error.codeExpired");
      case "too_many_attempts":
        return t("settings.account.error.tooManyAttempts");
      case "rate_limited":
        return t("settings.account.error.rateLimited");
      case "network_error":
        return t("settings.account.error.network");
      default:
        return error.message || t("settings.account.error.generic");
    }
  }
  return t("settings.account.error.generic");
}

// Persistent account control in the bottom-left rail (Codex-style). Signed out it
// shows "Sign in"; signed in it shows an avatar + email. Clicking opens a popover
// anchored above the button with the account surface.
export function AccountRailButton() {
  const t = useT();
  const status = useAuthStore((state) => state.status);
  const user = useAuthStore((state) => state.user);
  const hydrate = useAuthStore((state) => state.hydrate);

  useEffect(() => {
    if (useAuthStore.getState().status === "loading") {
      void hydrate();
    }
  }, [hydrate]);

  const signedIn = status === "signedIn" && !!user;
  const label = signedIn && user ? user.email : t("settings.account.signIn");

  return (
    <button
      className="rail-item"
      type="button"
      onClick={() => window.dispatchEvent(new Event("cerul:open-account"))}
      title={label}
    >
      <span className="rail-ind" aria-hidden="true" />
      {signedIn && user ? (
        <span className="rail-account-avatar" aria-hidden="true">
          {user.email.charAt(0).toUpperCase()}
        </span>
      ) : (
        <User size={17} />
      )}
      <span className="rail-label rail-account-label">{label}</span>
    </button>
  );
}

export function AccountDialogController() {
  const t = useT();
  const status = useAuthStore((state) => state.status);
  const user = useAuthStore((state) => state.user);
  const [open, setOpen] = useState(false);
  useEscapeToClose(() => setOpen(false), open);

  useEffect(() => {
    const onOpenRequest = () => setOpen(true);
    window.addEventListener("cerul:open-account", onOpenRequest);
    return () => window.removeEventListener("cerul:open-account", onOpenRequest);
  }, []);

  const signedIn = status === "signedIn" && !!user;
  const accountDialog = open ? (
    <>
      <div className="account-pop-backdrop" onClick={() => setOpen(false)} />
      <div
        className="account-pop"
        role="dialog"
        aria-modal="true"
        aria-label={t("settings.section.account")}
      >
        {signedIn ? <AccountSummary /> : <AccountAuthForm />}
      </div>
    </>
  ) : null;

  return accountDialog ? createPortal(accountDialog, document.body) : null;
}

function AccountAuthForm() {
  const t = useT();
  const login = useAuthStore((state) => state.login);
  const register = useAuthStore((state) => state.register);
  const [mode, setMode] = useState<AuthMode>("signin");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const switchMode = (next: AuthMode) => {
    setMode(next);
    setError(null);
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      if (mode === "signin") {
        await login({ email: email.trim(), password });
      } else {
        await register({ email: email.trim(), password });
      }
    } catch (err) {
      setError(friendlyError(err, t));
      setBusy(false);
    }
  };

  const startOAuth = async (provider: "google" | "github") => {
    setError(null);
    try {
      if (await startDesktopOAuth(provider)) {
        return;
      }
    } catch {
      setError(t("settings.account.error.network"));
      return;
    }
    const opened = window.open(cloudClient.oauthStartUrl(provider), "_blank", "noopener,noreferrer");
    if (!opened) {
      setError(t("settings.account.oauthPopupBlocked"));
    }
  };

  return (
    <div>
      <div className="account-pop-head">
        <div className="account-pop-title">
          <BrandMark className="account-pop-mark" />
          <span>Cerul Cloud</span>
        </div>
        <p className="account-pop-sub">{t("settings.account.subtitle")}</p>
      </div>
      <div className="account-pop-tabs">
        <button type="button" className={mode === "signin" ? "active" : ""} onClick={() => switchMode("signin")}>
          {t("settings.account.signIn")}
        </button>
        <button type="button" className={mode === "register" ? "active" : ""} onClick={() => switchMode("register")}>
          {t("settings.account.createAccount")}
        </button>
      </div>
      <form className="account-pop-form" onSubmit={submit}>
        <div className="account-field">
          <label className="field-label" htmlFor="rail-account-email">
            {t("settings.account.email")}
          </label>
          <input
            id="rail-account-email"
            className="input"
            type="email"
            autoComplete="email"
            value={email}
            disabled={busy}
            onChange={(event) => setEmail(event.currentTarget.value)}
          />
        </div>
        <div className="account-field">
          <label className="field-label" htmlFor="rail-account-password">
            {t("settings.account.password")}
          </label>
          <input
            id="rail-account-password"
            className="input"
            type="password"
            autoComplete={mode === "signin" ? "current-password" : "new-password"}
            value={password}
            disabled={busy}
            onChange={(event) => setPassword(event.currentTarget.value)}
          />
          {mode === "register" ? <p className="field-hint">{t("settings.account.passwordHint")}</p> : null}
        </div>
        {error ? <InlineNotice tone="error" message={error} /> : null}
        <button type="submit" className="btn btn-primary block" disabled={busy}>
          {mode === "signin" ? <LogIn size={16} /> : <UserPlus size={16} />}
          <span>
            {busy
              ? t("settings.account.working")
              : mode === "signin"
                ? t("settings.account.signIn")
                : t("settings.account.createAccount")}
          </span>
        </button>
      </form>
      <div className="account-or">{t("settings.account.or")}</div>
      <div className="account-oauth">
        <button type="button" className="btn btn-secondary block" disabled={busy} onClick={() => void startOAuth("google")}>
          <GoogleMark />
          <span>{t("settings.account.continueGoogle")}</span>
        </button>
        <button type="button" className="btn btn-secondary block" disabled={busy} onClick={() => void startOAuth("github")}>
          <Github size={16} />
          <span>{t("settings.account.continueGithub")}</span>
        </button>
      </div>
      <div className="account-reassure">
        <ShieldCheck size={14} />
        <span>{t("settings.account.reassure")}</span>
      </div>
    </div>
  );
}

function AccountSummary() {
  const t = useT();
  const user = useAuthStore((state) => state.user);
  const logout = useAuthStore((state) => state.logout);
  const [signingOut, setSigningOut] = useState(false);

  if (!user) {
    return null;
  }

  const doLogout = async () => {
    setSigningOut(true);
    try {
      await logout();
    } finally {
      setSigningOut(false);
    }
  };

  return (
    <div>
      <div className="account-pop-identity">
        <span className="account-pop-avatar" aria-hidden="true">
          {user.email.charAt(0).toUpperCase()}
        </span>
        <div className="account-pop-identity-text">
          <div className="account-pop-email">{user.email}</div>
          <div className="account-badges">
            <span className={`chip ${user.plan === "free" ? "neutral" : "accent"}`}>
              {t(`settings.account.plan.${user.plan}`)}
            </span>
            {user.email_verified ? (
              <span className="chip success">
                <CheckCircle2 size={13} />
                {t("settings.account.verified")}
              </span>
            ) : (
              <span className="chip warn">
                <AlertCircle size={13} />
                {t("settings.account.unverified")}
              </span>
            )}
          </div>
        </div>
      </div>
      {!user.email_verified ? <VerifyPanel /> : null}
      <div className="account-pop-foot">
        <button type="button" className="btn btn-secondary sm block" disabled={signingOut} onClick={() => void doLogout()}>
          <LogOut size={16} />
          <span>{t("settings.account.signOut")}</span>
        </button>
      </div>
    </div>
  );
}

function VerifyPanel() {
  const t = useT();
  const sendVerificationCode = useAuthStore((state) => state.sendVerificationCode);
  const verifyEmail = useAuthStore((state) => state.verifyEmail);
  const [code, setCode] = useState("");
  const [sending, setSending] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [notice, setNotice] = useState<{ tone: "error" | "muted"; message: string } | null>(null);

  const resend = async () => {
    setNotice(null);
    setSending(true);
    try {
      await sendVerificationCode();
      setNotice({ tone: "muted", message: t("settings.account.codeSent") });
    } catch (err) {
      setNotice({ tone: "error", message: friendlyError(err, t) });
    } finally {
      setSending(false);
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setNotice(null);
    setVerifying(true);
    try {
      await verifyEmail(code.trim());
    } catch (err) {
      setNotice({ tone: "error", message: friendlyError(err, t) });
      setVerifying(false);
    }
  };

  return (
    <div className="account-verify">
      <div className="account-verify-head">
        <Mail size={15} className="muted" />
        <span>{t("settings.account.verifyTitle")}</span>
      </div>
      <p className="field-hint account-verify-hint">{t("settings.account.verifyHint")}</p>
      <form className="account-verify-row" onSubmit={submit}>
        <input
          className="input account-code-input"
          inputMode="numeric"
          maxLength={6}
          placeholder="000000"
          aria-label={t("settings.account.codeAria")}
          value={code}
          disabled={verifying}
          onChange={(event) => setCode(event.currentTarget.value.replace(/\D/g, ""))}
        />
        <button type="submit" className="btn btn-primary sm" disabled={verifying || code.trim().length < 6}>
          <span>{verifying ? t("settings.account.working") : t("settings.account.verify")}</span>
        </button>
      </form>
      <button type="button" className="btn btn-ghost sm account-resend" disabled={sending} onClick={() => void resend()}>
        <span>{sending ? t("settings.account.working") : t("settings.account.resend")}</span>
      </button>
      {notice ? (
        <div className="account-verify-notice">
          <InlineNotice tone={notice.tone} message={notice.message} />
        </div>
      ) : null}
    </div>
  );
}
