// 引文卡工作台 — 2026-07-10 主题定稿（cerul-brand I_应用主题 §一.4）。
// 深底石墨卡（两主题同深底，分享卡 Q9-X3 的 app 内原位形态）：大字引文 +
// 铜色 mono 署名 + 单一复制动作。引文来自转写选区（选中即生成），
// 无选区时跟随当前播放句。

import { useCallback, useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
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
}: {
  title: string;
  link: string;
  draft: CitationDraft | null;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setCopied(false);
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
        </div>
      </div>
    </section>
  );
}
