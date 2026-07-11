// Collapsible "视频摘要" card for the ItemDetail redesign.
//
// Default (collapsed): a single slim row — summary badge + one-line
// lead (`summary` trimmed to one sentence) + inline keyword chips + a
// right-aligned "完整摘要 ▾" affordance. Clicking it inline-expands to
// the full summary card below (paragraph `summary` + keyword chips)
// without leaving the page.
//
// When the understanding record is missing / has no summary, the whole
// card is not rendered (returns null) so the layout never shows an empty
// placeholder.

import { useState } from "react";
import { Sparkles } from "lucide-react";
import { useT } from "../lib/i18n";

type SummaryCardProps = {
  summary: string | null;
  topics: string[];
  oneLiner?: string | null;
};

function firstSentence(text: string): string {
  if (!text) {
    return "";
  }
  // Take up to the first Chinese full stop / ASCII period / newline, then
  // trim to a reasonable single-line length so the collapsed row stays tidy.
  const match = text.match(/^[^。.。\n]+[。.。]?/);
  const head = (match ? match[0] : text).trim();
  if (head.length <= 96) {
    return head;
  }
  return `${head.slice(0, 95)}…`;
}

export function SummaryCard({
  summary,
  topics,
  oneLiner,
}: SummaryCardProps) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const cleanSummary = summary?.trim() ?? "";
  const lead = (oneLiner?.trim() || firstSentence(cleanSummary)).slice(0, 140);

  // No understanding / no summary → render nothing so the left rail keeps
  // flowing and we never show a blank card.
  if (!cleanSummary && topics.length === 0) {
    return null;
  }
  if (!lead) {
    return null;
  }

  return (
    <div className={open ? "speedread-shell is-open" : "speedread-shell"}>
      <div className="speedread speedread-collapsed">
        <div className="speedread-row">
          <span className="summary-ribbon">
            <Sparkles size={13} />
            {t("dt.summary.title")}
          </span>
          <span className="speedread-lead">{lead}</span>
          {topics.length > 0 ? (
            <span className="speedread-keys">
              {topics.slice(0, open ? 0 : 5).map((topic) => (
                <span key={topic} className="chip neutral keychip">
                  #{topic}
                </span>
              ))}
              {!open && topics.length > 5 ? (
                <span className="chip neutral keychip">+{topics.length - 5}</span>
              ) : null}
            </span>
          ) : null}
          <button
            type="button"
            className="btn btn-ghost sm speedread-toggle"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            title={open ? t("dt.summary.fullCollapse") : t("dt.summary.full")}
          >
            {open ? t("dt.summary.fullCollapse") : t("dt.summary.full")}
          </button>
        </div>
      </div>

      {open ? (
        <div className="speedread speedread-expanded" role="region" aria-label={t("dt.summary.title")}>
          <div className="speedread-head">
            <span className="summary-ribbon">
              <Sparkles size={13} />
              {t("dt.summary.title")}
            </span>
          </div>
          <p className="speedread-summary">{cleanSummary}</p>
          {topics.length > 0 ? (
            <div className="speedread-topics">
              {topics.map((topic) => (
                <span key={topic} className="chip neutral keychip">
                  #{topic}
                </span>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
