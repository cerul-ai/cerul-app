// 引文卡工作台 — 2026-07-10 主题定稿（cerul-brand I_应用主题 §一.4）。
// 深底石墨卡（两主题同深底，分享卡 Q9-X3 的 app 内原位形态）：大字引文 +
// 铜色 mono 署名 + 单一复制动作。引文来自转写选区（选中即生成），
// 无选区时跟随当前播放句。

import { useCallback, useEffect, useState } from "react";
import { AlertCircle, Check, Copy, Loader2 } from "lucide-react";
import { useT } from "../lib/i18n";
import { buildMomentCitation } from "../lib/formatters";
import { writeClipboardText } from "../lib/clipboard";

export type CitationDraft = {
  quote: string;
  displayTime: string;
  source: "selection" | "playhead";
};

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
  const [status, setStatus] = useState<"idle" | "working" | "copied" | "error">("idle");

  useEffect(() => {
    setStatus("idle");
  }, [draft?.quote, draft?.displayTime]);

  const copy = useCallback(async () => {
    if (!draft || status === "working") return;
    setStatus("working");
    try {
      let sharedLink: string | null = null;
      if (onShare) {
        try {
          sharedLink = await onShare();
        } catch {
          // Public sharing is an enhancement to citation copy. If Cloud is
          // unavailable, keep the local/original citation usable.
          sharedLink = null;
        }
      }
      const citationText = buildMomentCitation({
        title,
        timestamp: draft.displayTime,
        quote: draft.quote,
        link: sharedLink ?? link,
      });
      await writeClipboardText(citationText);
      setStatus("copied");
      setTimeout(() => setStatus("idle"), 1600);
    } catch {
      setStatus("error");
      setTimeout(() => setStatus("idle"), 2200);
    }
  }, [draft, link, onShare, status, title]);

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
          <button className="cite-btn pri" type="button" disabled={status === "working"} onClick={() => void copy()}>
            {status === "working" ? <Loader2 size={13} className="spin" /> : status === "copied" ? <Check size={13} /> : status === "error" ? <AlertCircle size={13} /> : <Copy size={13} />}
            {status === "working" ? t("detail.share.creating") : status === "copied" ? t("detail.copy.copied") : status === "error" ? t("detail.share.failedShort") : t("detail.copy.label")}
          </button>
        </div>
      </div>
    </section>
  );
}
