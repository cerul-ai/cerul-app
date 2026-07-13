import {
  Check,
  Copy,
  Loader2,
  LayoutGrid,
  List,
  Plus,
  RefreshCcw,
  Search,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ItemCard } from "../components/cards";
import { EmptyState, InlineNotice } from "../components/leaf";
import * as api from "../lib/api";
import { errorMessage } from "../lib/formatters";
import { itemStatus } from "../lib/items";
import { durationMinutes, sortLibraryItems } from "../lib/library";
import { writeClipboardText } from "../lib/clipboard";
import { useT } from "../lib/i18n";
import type { Item, RequestConfirm } from "../lib/types";

type LibrarySourceFilter = "all" | "local" | "bilibili" | "youtube" | "podcast" | "web";
type LibraryDurationFilter = "all" | "short" | "mid" | "long" | "xl";
type LibraryDateFilter = "all" | "week" | "month" | "older";
type LibraryContentFilter = "all" | "video" | "audio" | "document" | "image";

function librarySourceCategory(item: Item): Exclude<LibrarySourceFilter, "all"> {
  if (item.sourceKind === "folder") return "local";
  if (item.sourceKind === "youtube") return "youtube";
  if (item.sourceKind === "podcast") return "podcast";
  if (/bilibili/i.test(item.source) || /bilibili\.com/i.test(item.originalUrl ?? "")) return "bilibili";
  if (/youtube|youtu\.be/i.test(item.source) || /youtube\.com|youtu\.be/i.test(item.originalUrl ?? "")) return "youtube";
  return "web";
}

function itemDuration(item: Item): number | null {
  if (item.contentType !== "video" && item.contentType !== "audio") return null;
  if (typeof item.durationSec === "number" && Number.isFinite(item.durationSec)) return item.durationSec / 60;
  const formattedDuration = item.duration.trim();
  const hasColonDuration = /^\d+(?::\d+){1,2}$/.test(formattedDuration);
  const hasLegacyDurationUnit = /\d+\s*[hm]\b/i.test(formattedDuration);
  if (!hasColonDuration && !hasLegacyDurationUnit) return null;
  const minutes = durationMinutes(item.duration);
  return Number.isFinite(minutes) ? minutes : null;
}

function matchesDuration(item: Item, filter: LibraryDurationFilter): boolean {
  if (filter === "all") return true;
  const minutes = itemDuration(item);
  if (minutes === null) return false;
  if (filter === "short") return minutes < 5;
  if (filter === "mid") return minutes >= 5 && minutes < 20;
  if (filter === "long") return minutes >= 20 && minutes < 60;
  return minutes >= 60;
}

function matchesDate(item: Item, filter: LibraryDateFilter): boolean {
  if (filter === "all") return true;
  if (item.indexedAtEpoch === null) return false;
  const ageDays = (Date.now() / 1000 - item.indexedAtEpoch) / 86400;
  if (filter === "week") return ageDays <= 7;
  if (filter === "month") return ageDays <= 30;
  return ageDays > 30;
}

export function LibraryScreen({
  items,
  actionsEnabled,
  onAddSource,
  onDeleteItems,
  onReindexItems,
  onOpenItem,
  requestConfirm,
}: {
  items: Item[];
  actionsEnabled: boolean;
  onAddSource: () => void;
  onDeleteItems: (
    itemIds: string[],
    onProgress?: (completed: number, total: number) => void,
    options?: { keepDiscoverable?: boolean },
  ) => Promise<void>;
  onReindexItems: (itemIds: string[]) => Promise<void>;
  onOpenItem: (item: Item) => void;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [libraryQuery, setLibraryQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState<LibrarySourceFilter>("all");
  const [statusFilter, setStatusFilter] = useState<"all" | Item["status"]>("all");
  const [durationFilter, setDurationFilter] = useState<LibraryDurationFilter>("all");
  const [dateFilter, setDateFilter] = useState<LibraryDateFilter>("all");
  const [contentFilter, setContentFilter] = useState<LibraryContentFilter>("all");
  const [sortKey, setSortKey] = useState<"recent" | "longest" | "shortest" | "title">("recent");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [selectedItemIds, setSelectedItemIds] = useState<Set<string>>(new Set());
  const [batchState, setBatchState] = useState<{
    status: "idle" | "reindexing" | "deleting" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const [failedCleanupIds, setFailedCleanupIds] = useState<string[]>([]);
  const sourceCounts = useMemo(() => {
    const counts = new Map<Exclude<LibrarySourceFilter, "all">, number>();
    for (const item of items) {
      const category = librarySourceCategory(item);
      counts.set(category, (counts.get(category) ?? 0) + 1);
    }
    return counts;
  }, [items]);
  const statusCounts = useMemo(() => ({
    indexed: items.filter((item) => item.status === "indexed").length,
    indexing: items.filter((item) => item.status === "indexing").length,
    failed: items.filter((item) => item.status === "failed").length,
  }), [items]);
  const itemStatusSignature = useMemo(
    () => items.map((item) => `${item.id}:${item.status}`).join("|"),
    [items],
  );
  const normalizedQuery = libraryQuery.trim().toLowerCase();
  const filtersActive =
    normalizedQuery !== "" ||
    sourceFilter !== "all" ||
    statusFilter !== "all" ||
    durationFilter !== "all" ||
    dateFilter !== "all" ||
    contentFilter !== "all" ||
    sortKey !== "recent";
  const filteredItems = items
    .filter((item) => {
      const matchesQuery =
        normalizedQuery === "" ||
        item.title.toLowerCase().includes(normalizedQuery) ||
        item.source.toLowerCase().includes(normalizedQuery);
      const matchesSource = sourceFilter === "all" || librarySourceCategory(item) === sourceFilter;
      const matchesStatus = statusFilter === "all" || item.status === statusFilter;
      const matchesContent = contentFilter === "all" || item.contentType === contentFilter;
      return matchesQuery && matchesSource && matchesStatus && matchesContent && matchesDuration(item, durationFilter) && matchesDate(item, dateFilter);
    })
    .sort((a, b) => sortLibraryItems(a, b, sortKey));
  const selectedCount = selectedItemIds.size;
  const selectedItems = items.filter((item) => selectedItemIds.has(item.id));
  const filteredItemIds = filteredItems.map((item) => item.id);
  const visibleSelectedCount = filteredItemIds.filter((itemId) => selectedItemIds.has(itemId)).length;
  const allFilteredSelected = filteredItemIds.length > 0 && visibleSelectedCount === filteredItemIds.length;
  const batchPending = batchState.status === "reindexing" || batchState.status === "deleting";
  const failedCleanupCount = failedCleanupIds.length;

  useEffect(() => {
    const itemIds = new Set(items.map((item) => item.id));
    setSelectedItemIds((current) => {
      const next = new Set(Array.from(current).filter((itemId) => itemIds.has(itemId)));
      return next.size === current.size ? current : next;
    });
  }, [items]);

  function clearLibraryFilters() {
    setLibraryQuery("");
    setSourceFilter("all");
    setStatusFilter("all");
    setDurationFilter("all");
    setDateFilter("all");
    setContentFilter("all");
    setSortKey("recent");
  }

  function toggleItemSelection(itemId: string, selected: boolean) {
    setBatchState({ status: "idle", message: null });
    setSelectedItemIds((current) => {
      const next = new Set(current);
      if (selected) {
        next.add(itemId);
      } else {
        next.delete(itemId);
      }
      return next;
    });
  }

  function toggleAllFilteredItems() {
    setBatchState({ status: "idle", message: null });
    setSelectedItemIds((current) => {
      const next = new Set(current);
      if (allFilteredSelected) {
        for (const itemId of filteredItemIds) {
          next.delete(itemId);
        }
      } else {
        for (const itemId of filteredItemIds) {
          next.add(itemId);
        }
      }
      return next;
    });
  }

  async function runBatchAction(action: "reindex" | "delete") {
    if (!actionsEnabled) {
      setBatchState({
        status: "error",
        message: t("common.coreUnreachable"),
      });
      return;
    }

    const itemIds = Array.from(selectedItemIds);
    if (itemIds.length === 0) {
      return;
    }
    if (action === "delete") {
      const confirmed = await requestConfirm({
        title: t("library.batch.confirm.title"),
        body: t("library.batch.confirm.body", { count: itemIds.length }),
        confirmLabel: t("library.batch.confirm.label"),
      });
      if (!confirmed) {
        return;
      }
    }

    setBatchState({
      status: action === "delete" ? "deleting" : "reindexing",
      message:
        action === "delete"
          ? t("library.batch.deletingProgress", { completed: 0, total: itemIds.length })
          : null,
    });
    try {
      if (action === "delete") {
        await onDeleteItems(itemIds, (completed, total) => {
          setBatchState({
            status: "deleting",
            message: t("library.batch.deletingProgress", { completed, total }),
          });
        });
      } else {
        await onReindexItems(itemIds);
      }
      setSelectedItemIds(new Set());
      setBatchState({ status: "idle", message: null });
    } catch (error) {
      setBatchState({ status: "error", message: errorMessage(error) });
    }
  }

  async function copySelectedDiagnostics() {
    if (selectedItems.length === 0) {
      return;
    }
    const payload = {
      generated_at: new Date().toISOString(),
      selected_count: selectedItems.length,
      items: selectedItems.map((item) => ({
        id: item.id,
        title: item.title,
        source_id: item.sourceId,
        source: item.source,
        source_kind: item.sourceKind,
        content_type: item.contentType,
        status: item.status,
        indexed_at: item.indexedAtEpoch,
        raw_path: item.rawPath,
        raw_path_exists: item.rawPathExists,
        original_url: item.originalUrl,
        visual_index_status: item.visualIndexStatus,
        visual_index_message: item.visualIndexMessage,
        embedding_index_status: item.embeddingIndexStatus,
        embedding_index_message: item.embeddingIndexMessage,
        has_audio: item.hasAudio,
        usage: item.usage,
        error: item.error,
      })),
    };
    try {
      await writeClipboardText(JSON.stringify(payload, null, 2));
      setBatchState({
        status: "idle",
        message: t("library.batch.diagnosticsCopied", { count: selectedItems.length }),
      });
    } catch (error) {
      setBatchState({ status: "error", message: errorMessage(error) });
    }
  }

  async function collectAllFailedItemIds(): Promise<string[]> {
    const pageSize = 1000;
    const ids: string[] = [];
    for (let cursor = 0; ; cursor += pageSize) {
      const page = await api.listItems({
        status: "failed",
        limit: pageSize,
        cursor,
      });
      for (const item of page) {
        if (itemStatus(item) === "failed") {
          ids.push(item.id);
        }
      }
      if (page.length < pageSize) {
        break;
      }
    }
    return ids;
  }

  useEffect(() => {
    let cancelled = false;
    if (!actionsEnabled) {
      setFailedCleanupIds([]);
      return;
    }
    collectAllFailedItemIds()
      .then((ids) => {
        if (!cancelled) {
          setFailedCleanupIds(ids);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setFailedCleanupIds([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [actionsEnabled, itemStatusSignature]);

  async function clearFailedItems() {
    if (!actionsEnabled) {
      setBatchState({ status: "error", message: t("common.coreUnreachable") });
      return;
    }
    let ids: string[];
    try {
      ids = await collectAllFailedItemIds();
    } catch (error) {
      setBatchState({ status: "error", message: errorMessage(error) });
      return;
    }
    if (ids.length === 0) {
      setFailedCleanupIds([]);
      setBatchState({ status: "idle", message: null });
      return;
    }
    const confirmed = await requestConfirm({
      title: t("library.clearFailed.confirm.title"),
      body: t("library.clearFailed.confirm.body", { count: ids.length }),
      confirmLabel: t("library.clearFailed.confirm.label"),
    });
    if (!confirmed) {
      return;
    }
    setBatchState({
      status: "deleting",
      message: t("library.batch.deletingProgress", { completed: 0, total: ids.length }),
    });
    try {
      await onDeleteItems(
        ids,
        (completed, total) => {
          setBatchState({
            status: "deleting",
            message: t("library.batch.deletingProgress", { completed, total }),
          });
        },
        { keepDiscoverable: true },
      );
      setSelectedItemIds((prev) => {
        const next = new Set(prev);
        ids.forEach((id) => next.delete(id));
        return next;
      });
      setFailedCleanupIds([]);
      setBatchState({ status: "idle", message: null });
    } catch (error) {
      setBatchState({ status: "error", message: errorMessage(error) });
    }
  }

  const activeFilterCount = [sourceFilter, statusFilter, durationFilter, dateFilter, contentFilter]
    .filter((value) => value !== "all").length;
  const sourceFilterOptions: Array<{ value: Exclude<LibrarySourceFilter, "all">; label: string }> = [
    { value: "local", label: t("library.filter.source.local") },
    { value: "bilibili", label: "Bilibili" },
    { value: "youtube", label: "YouTube" },
    { value: "podcast", label: t("library.filter.source.podcast") },
    { value: "web", label: t("library.filter.source.web") },
  ];
  const durationFilterOptions: Array<{ value: Exclude<LibraryDurationFilter, "all">; label: string }> = [
    { value: "short", label: t("library.filter.duration.short") },
    { value: "mid", label: t("library.filter.duration.mid") },
    { value: "long", label: t("library.filter.duration.long") },
    { value: "xl", label: t("library.filter.duration.xl") },
  ];
  const dateFilterOptions: Array<{ value: Exclude<LibraryDateFilter, "all">; label: string }> = [
    { value: "week", label: t("library.filter.date.week") },
    { value: "month", label: t("library.filter.date.month") },
    { value: "older", label: t("library.filter.date.older") },
  ];
  const contentFilterOptions: Array<{ value: Exclude<LibraryContentFilter, "all">; label: string }> = [
    { value: "video", label: t("library.filter.content.video") },
    { value: "audio", label: t("library.filter.content.audio") },
    { value: "document", label: t("library.filter.content.document") },
    { value: "image", label: t("library.filter.content.image") },
  ];

  function switchLibraryView(nextView: "grid" | "list") {
    if (viewMode === nextView) return;
    const transitionDocument = document as Document & {
      startViewTransition?: (update: () => void) => unknown;
    };
    if (transitionDocument.startViewTransition && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      transitionDocument.startViewTransition(() => setViewMode(nextView));
      return;
    }
    setViewMode(nextView);
  }

  return (
    <div className="page wide library-retrieval-page library-final-page">
      <header className="library-final-head" aria-labelledby="library-retrieval-title">
        <div>
          <p className="page-eyebrow">{t("library.retrieval.eyebrow")}</p>
          <h1 className="page-h1" id="library-retrieval-title">{t("library.final.title")}</h1>
          <p className="page-sub">{t("library.final.body")}</p>
        </div>
        <div className="library-view-switch" role="group" aria-label={t("library.view.aria")}>
          <button type="button" className={viewMode === "grid" ? "active" : ""} aria-pressed={viewMode === "grid"} onClick={() => switchLibraryView("grid")}><LayoutGrid size={14} />{t("library.view.gridShort")}</button>
          <button type="button" className={viewMode === "list" ? "active" : ""} aria-pressed={viewMode === "list"} onClick={() => switchLibraryView("list")}><List size={14} />{t("library.view.listShort")}</button>
        </div>
      </header>

      <div className="library-final-layout">
        <aside className="library-filter-rail" aria-label={t("library.filter.aria")}>
          <header><h2>{t("library.filter.title")}</h2>{activeFilterCount > 0 ? <span className="mono">{t("library.filter.active", { count: activeFilterCount })}</span> : null}</header>
          <section>
            <h3>{t("library.filter.duration")}</h3>
            <button type="button" className={durationFilter === "all" ? "active" : ""} onClick={() => setDurationFilter("all")}>{t("library.filter.all")}</button>
            {durationFilterOptions.map((option) => <button type="button" className={durationFilter === option.value ? "active" : ""} key={option.value} onClick={() => setDurationFilter(option.value)}>{option.label}</button>)}
          </section>
          <section>
            <h3>{t("library.filter.date")}</h3>
            <button type="button" className={dateFilter === "all" ? "active" : ""} onClick={() => setDateFilter("all")}>{t("library.filter.all")}</button>
            {dateFilterOptions.map((option) => <button type="button" className={dateFilter === option.value ? "active" : ""} key={option.value} onClick={() => setDateFilter(option.value)}>{option.label}</button>)}
          </section>
          <section>
            <h3>{t("library.filter.sourceAria")}</h3>
            <button type="button" className={sourceFilter === "all" ? "active" : ""} onClick={() => setSourceFilter("all")}><span>{t("library.filter.allSources")}</span><code>{items.length}</code></button>
            {sourceFilterOptions.map((option) => <button type="button" className={sourceFilter === option.value ? "active" : ""} key={option.value} onClick={() => setSourceFilter(option.value)}><span>{option.label}</span><code>{sourceCounts.get(option.value) ?? 0}</code></button>)}
          </section>
          <section>
            <h3>{t("library.filter.content")}</h3>
            <button type="button" className={contentFilter === "all" ? "active" : ""} onClick={() => setContentFilter("all")}>{t("library.filter.all")}</button>
            {contentFilterOptions.map((option) => <button type="button" className={contentFilter === option.value ? "active" : ""} key={option.value} onClick={() => setContentFilter(option.value)}>{option.label}</button>)}
          </section>
          <section>
            <h3>{t("library.filter.statusAria")}</h3>
            <button type="button" className={statusFilter === "all" ? "active" : ""} onClick={() => setStatusFilter("all")}><span>{t("library.filter.allStatuses")}</span><code>{items.length}</code></button>
            {(["indexed", "indexing", "failed"] as const).map((status) => <button type="button" key={status} className={statusFilter === status ? "active" : ""} onClick={() => setStatusFilter(status)}><span>{t(`library.status.${status}`)}</span><code>{statusCounts[status]}</code></button>)}
          </section>
        </aside>

        <main className="library-final-panel card" data-view={viewMode}>
          <div className="library-final-toolbar">
            <label className="library-retrieval-search">
              <Search size={17} aria-hidden="true" />
              <input value={libraryQuery} placeholder={t("library.searchPlaceholder")} aria-label={t("library.searchPlaceholder")} onChange={(event) => setLibraryQuery(event.currentTarget.value)} />
            </label>
            <span className="library-result-count mono">{t("library.final.items", { count: filteredItems.length })}</span>
            <select className="select" aria-label={t("library.sort.aria")} value={sortKey} onChange={(event) => setSortKey(event.currentTarget.value as "recent" | "longest" | "shortest" | "title")}>
              <option value="recent">{t("library.sort.recent")}</option>
              <option value="longest">{t("library.sort.longest")}</option>
              <option value="shortest">{t("library.sort.shortest")}</option>
              <option value="title">{t("library.sort.title")}</option>
            </select>
            <button className="btn btn-secondary sm" type="button" onClick={onAddSource}><Plus size={14} />{t("home.addSource")}</button>
          </div>
          <div className="library-view-context">
            <span><strong>{t(viewMode === "grid" ? "library.view.grid" : "library.view.list")}</strong> · {t(viewMode === "grid" ? "library.view.gridHint" : "library.view.listHint")}</span>
            <span className="mono">{t("library.view.motionHint")}</span>
          </div>
          <div className="library-final-actions">
            <span>{t("library.summary.count", { count: filteredItems.length, total: items.length })}</span>
            {filtersActive ? <button type="button" className="btn btn-ghost sm" onClick={clearLibraryFilters}>{t("common.clearFilters")}</button> : null}
            {filteredItems.length > 0 ? <button type="button" className="btn btn-ghost sm library-select-all" disabled={batchPending} onClick={toggleAllFilteredItems}><Check size={14} />{allFilteredSelected ? t("library.batch.selectNone") : t("library.batch.selectAll")}</button> : null}
            {failedCleanupCount > 0 ? <button type="button" className="btn btn-ghost sm danger-text" disabled={batchPending || !actionsEnabled} onClick={() => void clearFailedItems()} title={t("library.clearFailed.hint")}><Trash2 size={14} />{t("library.clearFailed.button", { count: failedCleanupCount })}</button> : null}
          </div>
          {batchState.message ? <InlineNotice tone={batchState.status === "error" ? "error" : "muted"} message={batchState.message} /> : null}
          {selectedCount > 0 ? (
            <div className="library-batch-toolbar" aria-label={t("library.batch.aria")}>
              <span className="chip accent"><span className="dot" />{t("library.batch.selected", { count: selectedCount })}</span><span className="grow" />
              <button type="button" className="btn btn-secondary sm" disabled={batchPending} onClick={() => void copySelectedDiagnostics()}><Copy size={15} />{t("library.batch.copyDiagnostics")}</button>
              <button type="button" className="btn btn-secondary sm" disabled={batchPending || !actionsEnabled} onClick={() => void runBatchAction("reindex")}>{batchState.status === "reindexing" ? <Loader2 size={15} className="spin" /> : <RefreshCcw size={15} />}{batchState.status === "reindexing" ? t("common.reindexing") : t("common.reindex")}</button>
              <button type="button" className="btn btn-danger sm" disabled={batchPending || !actionsEnabled} onClick={() => void runBatchAction("delete")}>{batchState.status === "deleting" ? <Loader2 size={15} className="spin" /> : <Trash2 size={15} />}{batchState.status === "deleting" ? t("common.deleting") : t("common.delete")}</button>
              <button type="button" className="btn btn-ghost sm" disabled={batchPending} onClick={() => setSelectedItemIds(new Set())}>{t("library.batch.clear")}</button>
            </div>
          ) : null}
          {items.length > 0 && filteredItems.length > 0 ? (
            <div className={viewMode === "grid" ? "lib-grid library-l3-grid library-view-collection" : "tbl lib-table library-retrieval-table library-view-collection"}>
              {viewMode === "list" ? <div className="lib-table-head" aria-hidden="true"><span>{t("library.col.title")}</span><span>{t("library.col.source")}</span><span>{t("library.col.duration")}</span><span>{t("library.col.indexed")}</span><span>{t("library.col.status")}</span></div> : null}
              {filteredItems.map((item, index) => <ItemCard key={item.id} item={item} viewMode={viewMode} transitionName={`library-item-${index}`} selectable selected={selectedItemIds.has(item.id)} onSelect={(selected) => toggleItemSelection(item.id, selected)} onOpen={() => onOpenItem(item)} />)}
            </div>
          ) : items.length === 0 ? (
            <EmptyState title={t("library.empty.none.title")} body={t("library.empty.none.body")} actionLabel={t("library.empty.addSource")} onAction={onAddSource} />
          ) : (
            <EmptyState title={t("library.empty.filtered.title")} body={t("library.empty.filtered.body")} actionLabel={t("common.clearFilters")} onAction={clearLibraryFilters} />
          )}
        </main>
      </div>
    </div>
  );
}
