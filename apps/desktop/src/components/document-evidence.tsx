import { ExternalLink, FileText } from "lucide-react";
import type * as api from "../lib/api";
import { canOpenOriginalSource } from "../lib/detail";
import {
  documentChunkLabel,
} from "../lib/results";
import { useT } from "../lib/i18n";
import type { Item } from "../lib/types";

export function DocumentEvidencePanel({
  item,
  chunk,
  chunkCount,
  matchedSnippet,
  onOpenOriginal,
}: {
  item: Item;
  chunk: api.ChunkRecord | null;
  chunkCount: number;
  matchedSnippet?: string | null;
  onOpenOriginal: () => void;
}) {
  const t = useT();
  const snippet = matchedSnippet?.trim() || chunk?.text?.trim() || "";

  return (
    <section className="document-evidence-panel" aria-label={t("detail.document.label")}>
      <div className="document-evidence-icon" aria-hidden="true">
        <FileText size={28} />
      </div>
      <div className="document-evidence-body">
        <p className="section-label">{t("detail.document.label")}</p>
        <strong className="document-evidence-title">{item.title}</strong>
        <div className="row gap-2 document-evidence-meta">
          <span className="chip neutral">{t("detail.document.chunkCount", { count: chunkCount })}</span>
          {chunk ? <span className="chip accent">{documentChunkLabel(chunk, t)}</span> : null}
        </div>
        {snippet ? <p className="document-evidence-snippet">{snippet}</p> : null}
        <button
          className="btn btn-secondary sm"
          type="button"
          disabled={!canOpenOriginalSource(item)}
          onClick={onOpenOriginal}
        >
          <ExternalLink size={15} />
          <span>{t("detail.document.openOriginal")}</span>
        </button>
      </div>
    </section>
  );
}
