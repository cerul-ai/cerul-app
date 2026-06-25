import { useEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { AlertCircle, CheckCircle2, LogOut, Mail, User } from "lucide-react";
import { useT, type TFunction } from "../lib/i18n";
import { useDialogFocus, useEscapeToClose } from "../lib/use-dismissable";
import { InlineNotice } from "./leaf";
import { AuthModal, useResolvedTheme, type AuthMode } from "./auth-modal";
import { useAuthStore } from "../lib/cloud/authStore";
import { cloudClient } from "../lib/cloud/client";
import { CloudApiError } from "../lib/cloud/types";
import { startDesktopOAuth } from "../lib/desktopHost";

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
  const hydrate = useAuthStore((state) => state.hydrate);
  const [open, setOpen] = useState(false);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const close = () => setOpen(false);
  const signedIn = status === "signedIn" && !!user;
  useEscapeToClose(close, open);
  // Focus-trap the signed-in popover; the auth modal manages its own focus.
  useDialogFocus(dialogRef, open && signedIn);

  useEffect(() => {
    if (useAuthStore.getState().status === "loading") {
      void hydrate();
    }
  }, [hydrate]);

  useEffect(() => {
    const onOpenRequest = () => setOpen(true);
    window.addEventListener("cerul:open-account", onOpenRequest);
    return () => window.removeEventListener("cerul:open-account", onOpenRequest);
  }, []);

  if (!open) {
    return null;
  }

  // Signed in: the compact account popover. Signed out: the full-screen
  // login / register modal (which renders its own backdrop + scrim).
  const surface = signedIn ? (
    <>
      <div className="account-pop-backdrop" onClick={close} />
      <div
        ref={dialogRef}
        className="account-pop"
        role="dialog"
        aria-modal="true"
        aria-label={t("settings.section.account")}
      >
        <AccountSummary />
      </div>
    </>
  ) : (
    <AccountAuthForm onClose={close} />
  );

  return createPortal(surface, document.body);
}

function AccountAuthForm({ onClose }: { onClose: () => void }) {
  const t = useT();
  const theme = useResolvedTheme();
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
    <AuthModal
      theme={theme}
      mode={mode}
      email={email}
      password={password}
      busy={busy}
      error={error}
      t={t}
      onModeChange={switchMode}
      onEmailChange={setEmail}
      onPasswordChange={setPassword}
      onSubmit={submit}
      onGoogle={() => void startOAuth("google")}
      onGithub={() => void startOAuth("github")}
      onClose={onClose}
    />
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
