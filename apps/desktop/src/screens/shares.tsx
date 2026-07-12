import { Copy, ExternalLink, Link2, LogIn, ShieldCheck, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { writeClipboardText } from "../lib/clipboard";
import { cloudClient } from "../lib/cloud/client";
import { useAuthStore } from "../lib/cloud/authStore";
import { useLang, useT } from "../lib/i18n";
import {
  markManagedShareRevoked,
  readManagedShares,
  type ManagedShare,
} from "../lib/managed-shares";
import type { RequestConfirm } from "../lib/types";
import { EmptyState } from "../components/leaf";

type ShareFilter = "active" | "revoked" | "all";

export function SharesScreen({ requestConfirm }: { requestConfirm: RequestConfirm }) {
  const t = useT();
  const { lang } = useLang();
  const authStatus = useAuthStore((state) => state.status);
  const accessToken = useAuthStore((state) => state.accessToken);
  const [shares, setShares] = useState<ManagedShare[]>(() => readManagedShares());
  const [filter, setFilter] = useState<ShareFilter>("active");
  const [selectedId, setSelectedId] = useState<string | null>(() =>
    readManagedShares().find((share) => share.status === "active")?.id ?? readManagedShares()[0]?.id ?? null,
  );
  const [busyId, setBusyId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const counts = useMemo(
    () => ({
      all: shares.length,
      active: shares.filter((share) => share.status === "active").length,
      revoked: shares.filter((share) => share.status === "revoked").length,
    }),
    [shares],
  );
  const filteredShares = shares.filter((share) => filter === "all" || share.status === filter);
  const selectedShare = shares.find((share) => share.id === selectedId) ?? filteredShares[0] ?? null;

  function chooseFilter(next: ShareFilter) {
    setFilter(next);
    const first = shares.find((share) => next === "all" || share.status === next);
    setSelectedId(first?.id ?? null);
    setNotice(null);
  }

  async function copyShare(share: ManagedShare) {
    try {
      await writeClipboardText(share.share_url);
      setNotice(t("shares.notice.copied"));
    } catch {
      setNotice(t("shares.notice.copyError"));
    }
    window.setTimeout(() => setNotice(null), 1600);
  }

  async function revokeShare(share: ManagedShare) {
    if (!accessToken) {
      window.dispatchEvent(new CustomEvent("cerul:open-account"));
      return;
    }
    const confirmed = await requestConfirm({
      title: t("shares.revoke.title"),
      body: t("shares.revoke.body"),
      confirmLabel: t("shares.revoke.action"),
    });
    if (!confirmed) return;
    setBusyId(share.id);
    setNotice(null);
    try {
      await cloudClient.revokeShare(accessToken, share.id);
      setShares(markManagedShareRevoked(share.id));
      setFilter("revoked");
      setNotice(t("shares.notice.revoked"));
    } catch {
      setNotice(t("shares.notice.revokeError"));
    } finally {
      setBusyId(null);
    }
  }

  const formatDate = (value: string) =>
    new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(value));

  return (
    <div className="page wide shares-page-p1">
      <header className="shares-page-head">
        <div>
          <p className="page-eyebrow">{t("shares.eyebrow")}</p>
          <h1>{t("shares.title")}</h1>
          <p>{t("shares.subtitle")}</p>
        </div>
        <span className="shares-scope-note"><ShieldCheck size={14} />{t("shares.scope")}</span>
      </header>

      <div className={selectedShare ? "shares-workspace has-inspector" : "shares-workspace empty-inspector"}>
        <aside className="shares-filter-rail" aria-label={t("shares.filter.aria")}>
          <h2>{t("shares.filter.title")}</h2>
          <button className={filter === "active" ? "active" : ""} type="button" onClick={() => chooseFilter("active")}>
            <span>{t("shares.filter.active")}</span><code>{counts.active}</code>
          </button>
          <button className={filter === "revoked" ? "active" : ""} type="button" onClick={() => chooseFilter("revoked")}>
            <span>{t("shares.filter.revoked")}</span><code>{counts.revoked}</code>
          </button>
          <button className={filter === "all" ? "active" : ""} type="button" onClick={() => chooseFilter("all")}>
            <span>{t("shares.filter.all")}</span><code>{counts.all}</code>
          </button>
          <div className="shares-ledger-summary">
            <span><b className="mono">{counts.all}</b><small>{t("shares.stats.created")}</small></span>
            <span><b className="mono">{counts.active}</b><small>{t("shares.stats.public")}</small></span>
          </div>
          <p>{t("shares.localOnly")}</p>
        </aside>

        <main className="shares-list" aria-label={t("shares.list.aria")}>
          {filteredShares.length > 0 ? filteredShares.map((share) => (
            <article
              key={share.id}
              className={share.id === selectedShare?.id ? "share-ledger-row active" : "share-ledger-row"}
              tabIndex={0}
              onClick={() => setSelectedId(share.id)}
              onKeyDown={(event) => {
                if (event.key === "Enter") setSelectedId(share.id);
              }}
            >
              <span className="share-ledger-poster">
                {share.status === "active" ? <img src={share.poster_url} alt={t("shares.posterAlt", { title: share.title })} /> : <Link2 size={20} aria-hidden="true" />}
              </span>
              <span className="share-ledger-copy">
                <small className="mono">{share.id} · S2</small>
                <strong className="clamp1">{share.title}</strong>
                <span className="clamp1">“{share.headline}”</span>
              </span>
              <span className={`share-ledger-status ${share.status}`}>
                {share.status === "active" ? t("shares.status.active") : t("shares.status.revoked")}
                <time className="mono">{formatDate(share.published_at)}</time>
              </span>
              <span className="share-ledger-open" aria-hidden="true">›</span>
            </article>
          )) : (
            <EmptyState
              title={filter === "revoked" ? t("shares.empty.revoked.title") : t("shares.empty.title")}
              body={filter === "revoked" ? t("shares.empty.revoked.body") : t("shares.empty.body")}
            />
          )}
        </main>

        <aside className={selectedShare ? "shares-inspector" : "shares-inspector shares-inspector--empty"}>
          {selectedShare ? (
            <>
              <header>
                <h2>{t("shares.detail.title")}</h2>
                <span className={`share-ledger-status ${selectedShare.status}`}>
                  {selectedShare.status === "active" ? t("shares.status.active") : t("shares.status.revoked")}
                </span>
              </header>
              <div className="shares-og-card">
                {selectedShare.status === "active" ? <img src={selectedShare.poster_url} alt={t("shares.posterAlt", { title: selectedShare.title })} /> : <Link2 size={28} aria-hidden="true" />}
                <span><small>CERUL · S2</small><strong>“{selectedShare.headline}”</strong></span>
              </div>
              <blockquote>“{selectedShare.headline}”</blockquote>
              <dl>
                <div><dt>{t("shares.detail.page")}</dt><dd className="mono">/s/{selectedShare.id}</dd></div>
                <div><dt>{t("shares.detail.access")}</dt><dd>{t("shares.detail.anyone")}</dd></div>
                <div><dt>{t("shares.detail.created")}</dt><dd>{formatDate(selectedShare.published_at)}</dd></div>
                <div><dt>{t("shares.detail.expiry")}</dt><dd>{t("shares.detail.permanent")}</dd></div>
              </dl>
              {selectedShare.status === "active" ? (
                <div className="shares-actions">
                  <button className="btn btn-primary" type="button" onClick={() => void copyShare(selectedShare)}>
                    <Copy size={14} />{t("shares.action.copy")}
                  </button>
                  <button className="btn btn-secondary" type="button" onClick={() => window.open(selectedShare.share_url, "_blank", "noopener,noreferrer")}>
                    <ExternalLink size={14} />{t("shares.action.preview")}
                  </button>
                  <button className="btn btn-ghost danger" type="button" disabled={busyId === selectedShare.id} onClick={() => void revokeShare(selectedShare)}>
                    <Trash2 size={14} />{busyId === selectedShare.id ? t("common.loading") : t("shares.action.revoke")}
                  </button>
                </div>
              ) : null}
            </>
          ) : authStatus === "signedOut" ? (
            <div className="shares-inspector-empty">
              <LogIn size={20} aria-hidden="true" />
              <strong>{t("shares.signIn.title")}</strong>
              <p>{t("shares.signIn.body")}</p>
              <button className="btn btn-primary" type="button" onClick={() => window.dispatchEvent(new CustomEvent("cerul:open-account"))}>{t("settings.account.signIn")}</button>
            </div>
          ) : (
            <div className="shares-inspector-empty"><Link2 size={20} aria-hidden="true" /><strong>{t("shares.empty.title")}</strong><p>{t("shares.empty.body")}</p></div>
          )}
          {notice ? <p className="shares-notice" role="status">{notice}</p> : null}
        </aside>
      </div>
    </div>
  );
}
