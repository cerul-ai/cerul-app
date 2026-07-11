// 引文卡工作台 — 2026-07-10 主题定稿（cerul-brand I_应用主题 §一.4）。
// 深底石墨卡（两主题同深底，分享卡 Q9-X3 的 app 内原位形态）：大字引文 +
// 铜色 mono 署名 + 动作行。引文来自转写选区（选中即生成），无选区时跟随
// 当前播放句。引用篮 v1 = localStorage，导出 = 拼接复制。

import { useCallback, useEffect, useState } from "react";
import { Check, Copy, ExternalLink, Inbox, Loader2, Plus, Share2 } from "lucide-react";
import { useT } from "../lib/i18n";
import { buildMomentCitation } from "../lib/formatters";
import { writeClipboardText } from "../lib/clipboard";

export type CitationDraft = {
  quote: string;
  displayTime: string;
  source: "selection" | "playhead";
};

const BASKET_STORE = "cerul.citationBasket.v1";

function loadBasket(): string[] {
  try {
    const raw = localStorage.getItem(BASKET_STORE);
    const parsed = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

function saveBasket(entries: string[]) {
  try {
    localStorage.setItem(BASKET_STORE, JSON.stringify(entries));
  } catch {
    // best-effort persistence; the in-memory basket still works this session
  }
}

export function CitationCard({
  title,
  link,
  draft,
  onShare,
}: {
  title: string;
  link: string;
  draft: CitationDraft | null;
  onShare?: () => Promise<string | null>;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const [added, setAdded] = useState(false);
  const [basket, setBasket] = useState<string[]>(() => loadBasket());
  const [exported, setExported] = useState(false);
  const [shareState, setShareState] = useState<"idle" | "sharing" | "shared" | "error">("idle");
  const [shareUrl, setShareUrl] = useState<string | null>(null);

  useEffect(() => {
    setCopied(false);
    setAdded(false);
    setShareState("idle");
    setShareUrl(null);
  }, [draft?.quote, draft?.displayTime]);

  const citationText = draft
    ? buildMomentCitation({
        title,
        timestamp: draft.displayTime,
        quote: draft.quote,
        link,
      })
    : null;

  const copy = useCallback(async () => {
    if (!citationText) return;
    try {
      await writeClipboardText(citationText);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      // clipboard errors surface via the header copy path; keep the card quiet
    }
  }, [citationText]);

  const addToBasket = useCallback(() => {
    if (!citationText) return;
    setBasket((prev) => {
      const next = prev.includes(citationText) ? prev : [...prev, citationText];
      saveBasket(next);
      return next;
    });
    setAdded(true);
    setTimeout(() => setAdded(false), 1600);
  }, [citationText]);

  const exportAll = useCallback(async () => {
    if (basket.length === 0) return;
    try {
      await writeClipboardText(basket.join("\n\n"));
      setExported(true);
      setTimeout(() => setExported(false), 1600);
    } catch {
      // ignore, same as copy
    }
  }, [basket]);

  const clearBasket = useCallback(() => {
    setBasket([]);
    saveBasket([]);
  }, []);

  const share = useCallback(async () => {
    if (!onShare || shareState === "sharing") return;
    setShareState("sharing");
    setShareUrl(null);
    try {
      const url = await onShare();
      if (!url) {
        setShareState("idle");
        return;
      }
      await writeClipboardText(url);
      setShareUrl(url);
      setShareState("shared");
    } catch {
      setShareState("error");
    }
  }, [onShare, shareState]);

  if (!draft) return null;

  return (
    <section className="cite-card" aria-label={t("detail.cite.label")}>
      <div className="cite-body">
        <div className="cite-label mono">
          {t("detail.cite.label")} ·{" "}
          {draft.source === "selection" ? t("detail.cite.fromSelection") : t("detail.cite.fromPlayhead")}{" "}
          <span className="cite-ts">{draft.displayTime}</span>
        </div>
        <blockquote className="cite-quote">“{draft.quote}”</blockquote>
        <div className="cite-by mono">
          — {title} · {draft.displayTime}
        </div>
        <div className="cite-actions">
          <button className="cite-btn pri" type="button" onClick={() => void copy()}>
            {copied ? <Check size={13} /> : <Copy size={13} />}
            {copied ? t("detail.copy.copied") : t("detail.copy.label")}
          </button>
          {onShare ? (
            <button className="cite-btn" type="button" disabled={shareState === "sharing"} onClick={() => void share()}>
              {shareState === "sharing" ? <Loader2 className="spin" size={13} /> : shareState === "shared" ? <Check size={13} /> : <Share2 size={13} />}
              {shareState === "sharing" ? t("detail.share.creating") : shareState === "shared" ? t("detail.share.copied") : t("detail.share.action")}
            </button>
          ) : null}
          <button className="cite-btn" type="button" onClick={addToBasket}>
            {added ? <Check size={13} /> : <Plus size={13} />}
            {added ? t("detail.cite.added") : t("detail.cite.add")}
          </button>
          {basket.length > 0 ? (
            <span className="cite-basket">
              <Inbox size={13} aria-hidden="true" />
              {t("detail.cite.basket")} {basket.length}
              <button className="cite-basket-btn" type="button" onClick={() => void exportAll()}>
                {exported ? t("detail.copy.copied") : t("detail.cite.exportAll")}
              </button>
              <button className="cite-basket-btn" type="button" onClick={clearBasket}>
                {t("detail.cite.clear")}
              </button>
            </span>
          ) : null}
        </div>
        {shareState === "error" ? <p className="cite-share-status error" role="alert">{t("detail.share.error")}</p> : null}
        {shareUrl ? <a className="cite-share-status" href={shareUrl} target="_blank" rel="noreferrer"><ExternalLink size={12} />{t("detail.share.open")}</a> : null}
      </div>
    </section>
  );
}
