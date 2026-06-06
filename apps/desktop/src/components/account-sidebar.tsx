import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Cloud,
  LogIn,
  LogOut,
  Mail,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  User,
  UserPlus,
} from "lucide-react";
import { useT, type TFunction } from "../lib/i18n";
import { InlineNotice } from "./leaf";
import { useAuthStore } from "../lib/cloud/authStore";
import { CloudApiError } from "../lib/cloud/types";

type AuthMode = "signin" | "register";

// What signing in unlocks — shown value-first on the signed-out popover
// (design Area 1, direction A). Static marketing copy; keys live in the catalog.
const ACCOUNT_VALUES: { icon: ReactNode; titleKey: string; descKey: string }[] = [
  {
    icon: <Cloud size={16} />,
    titleKey: "settings.account.value.credits.title",
    descKey: "settings.account.value.credits.desc",
  },
  {
    icon: <RefreshCw size={16} />,
    titleKey: "settings.account.value.sync.title",
    descKey: "settings.account.value.sync.desc",
  },
  {
    icon: <Sparkles size={16} />,
    titleKey: "settings.account.value.pro.title",
    descKey: "settings.account.value.pro.desc",
  },
];

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
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [pos, setPos] = useState({ left: 10, bottom: 56 });

  useEffect(() => {
    if (useAuthStore.getState().status === "loading") {
      void hydrate();
    }
  }, [hydrate]);

  // Anchor the popover just above the button, left-aligned (works at any size).
  useEffect(() => {
    if (open && buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect();
      setPos({ left: Math.round(rect.left), bottom: Math.round(window.innerHeight - rect.top + 8) });
    }
  }, [open]);

  const signedIn = status === "signedIn" && !!user;
  const label = signedIn && user ? user.email : t("settings.account.signIn");

  return (
    <>
      <button
        ref={buttonRef}
        className={open ? "rail-item active" : "rail-item"}
        type="button"
        onClick={() => setOpen((value) => !value)}
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
      {open ? (
        <>
          <div className="account-pop-backdrop" onClick={() => setOpen(false)} />
          <div
            className="account-pop"
            role="dialog"
            aria-label={t("settings.section.account")}
            style={{ left: pos.left, bottom: pos.bottom }}
          >
            {signedIn ? <AccountSummary /> : <AccountAuthForm />}
          </div>
        </>
      ) : null}
    </>
  );
}

function AccountAuthForm() {
  const t = useT();
  const login = useAuthStore((state) => state.login);
  const register = useAuthStore((state) => state.register);
  const [mode, setMode] = useState<AuthMode>("signin");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

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
        const trimmedName = name.trim();
        await register({ email: email.trim(), password, ...(trimmedName ? { name: trimmedName } : {}) });
      }
    } catch (err) {
      setError(friendlyError(err, t));
      setBusy(false);
    }
  };

  return (
    <div>
      <div className="account-pop-head">
        <div className="account-pop-title">Cerul Cloud</div>
        <p className="account-pop-sub">{t("settings.account.intro")}</p>
      </div>
      {!showForm ? (
        <>
          <div className="account-values">
            {ACCOUNT_VALUES.map((value) => (
              <div className="account-value-row" key={value.titleKey}>
                <span className="account-value-ico" aria-hidden="true">
                  {value.icon}
                </span>
                <div>
                  <div className="account-value-t">{t(value.titleKey)}</div>
                  <div className="account-value-d">{t(value.descKey)}</div>
                </div>
              </div>
            ))}
          </div>
          <div className="account-reassure">
            <ShieldCheck size={14} />
            <span>{t("settings.account.reassure")}</span>
          </div>
          <button
            type="button"
            className="btn btn-primary block account-reveal"
            onClick={() => setShowForm(true)}
          >
            <LogIn size={16} />
            <span>{t("settings.account.signInOrUp")}</span>
          </button>
        </>
      ) : (
        <>
          <div className="segmented account-pop-tabs">
            <button type="button" className={mode === "signin" ? "active" : ""} onClick={() => switchMode("signin")}>
              {t("settings.account.signIn")}
            </button>
            <button type="button" className={mode === "register" ? "active" : ""} onClick={() => switchMode("register")}>
              {t("settings.account.createAccount")}
            </button>
          </div>
          <form className="account-pop-form" onSubmit={submit}>
        {mode === "register" ? (
          <div className="account-field">
            <label className="field-label" htmlFor="rail-account-name">
              {t("settings.account.name")}
            </label>
            <input
              id="rail-account-name"
              className="input"
              autoComplete="name"
              value={name}
              disabled={busy}
              onChange={(event) => setName(event.currentTarget.value)}
            />
          </div>
        ) : null}
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
        </>
      )}
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
