import {
  AlertTriangle,
  Check,
  Copy,
  Eye,
  FileText,
  Library,
  ListFilter,
  Loader2,
  Mic,
  Plus,
  RefreshCcw,
  Search,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { IndexingStrip } from "../components/indexing-strip";
import { ItemCard } from "../components/cards";
import { EmptyState, InlineNotice } from "../components/leaf";
import * as api from "../lib/api";
import { errorMessage } from "../lib/formatters";
import {
  itemEmbeddingIndexStatus,
  itemHasAudio,
  itemHasPartialIndex,
  itemHasSpeechSearch,
  itemHasVisualSearch,
  itemStatus,
  itemVisualIndexStatus,
} from "../lib/items";
import { sortLibraryItems } from "../lib/library";
import { writeClipboardText } from "../lib/clipboard";
import { useT } from "../lib/i18n";
import type { Item, RequestConfirm, Source } from "../lib/types";

type LibraryCapabilityCounts = {
  document: number;
  speechOnly: number;
  visual: number;
  partial: number;
};

type LibraryCapabilityItem = Pick<
  Item,
  "contentType" | "embeddingIndexStatus" | "hasAudio" | "status" | "visualIndexStatus"
>;

function libraryCapabilityItemFromRecord(record: api.ItemRecord): LibraryCapabilityItem {
  return {
    contentType: record.content_type,
    embeddingIndexStatus: itemEmbeddingIndexStatus(record),
    hasAudio: itemHasAudio(record),
    status: itemStatus(record),
    visualIndexStatus: itemVisualIndexStatus(record),
  };
}

function countLibraryCapabilities(items: LibraryCapabilityItem[]): LibraryCapabilityCounts {
  const indexedItems = items.filter((item) => item.status === "indexed");
  return {
    document: indexedItems.filter((item) => item.contentType === "document").length,
    speechOnly: indexedItems.filter((item) => itemHasSpeechSearch(item) && !itemHasVisualSearch(item))
      .length,
    visual: indexedItems.filter(itemHasVisualSearch).length,
    partial: indexedItems.filter(itemHasPartialIndex).length,
  };
}

export function LibraryScreen({
  items,
  jobs,
  syncingSources,
  stepStarts,
  indexingPaused,
  actionsEnabled,
  onAddSource,
  onDeleteItems,
  onReindexItems,
  onOpenItem,
  onOpenJobs,
  requestConfirm,
}: {
  items: Item[];
  jobs: api.JobRecord[];
  syncingSources: Source[];
  stepStarts: Record<string, number>;
  indexingPaused: boolean;
  actionsEnabled: boolean;
  onAddSource: () => void;
  onDeleteItems: (
    itemIds: string[],
    onProgress?: (completed: number, total: number) => void,
    options?: { keepDiscoverable?: boolean },
  ) => Promise<void>;
  onReindexItems: (itemIds: string[]) => Promise<void>;
  onOpenItem: (item: Item) => void;
  onOpenJobs: () => void;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [libraryQuery, setLibraryQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");
  const [sortKey, setSortKey] = useState<"recent" | "longest" | "shortest" | "title">("recent");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [selectedItemIds, setSelectedItemIds] = useState<Set<string>>(new Set());
  const [batchState, setBatchState] = useState<{
    status: "idle" | "reindexing" | "deleting" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const [failedCleanupIds, setFailedCleanupIds] = useState<string[]>([]);
  const [allCapabilityCounts, setAllCapabilityCounts] = useState<LibraryCapabilityCounts | null>(null);
  const sourceOptions = Array.from(new Set(items.map((item) => item.source))).sort((a, b) =>
    a.localeCompare(b),
  );
  const itemStatusSignature = useMemo(
    () => items.map((item) => `${item.id}:${item.status}`).join("|"),
    [items],
  );
  const itemCapabilitySignature = useMemo(
    () =>
      items
        .map(
          (item) =>
            `${item.id}:${item.status}:${item.contentType}:${item.embeddingIndexStatus ?? ""}:${item.visualIndexStatus ?? ""}:${String(item.hasAudio)}`,
        )
        .join("|"),
    [items],
  );
  const jobStatusSignature = useMemo(
    () => jobs.map((job) => `${job.id}:${job.item_id ?? ""}:${job.status}`).join("|"),
    [jobs],
  );
  const normalizedQuery = libraryQuery.trim().toLowerCase();
  const filtersActive =
    normalizedQuery !== "" ||
    sourceFilter !== "all" ||
    statusFilter !== "all" ||
    sortKey !== "recent";
  const filteredItems = items
    .filter((item) => {
      const matchesQuery =
        normalizedQuery === "" ||
        item.title.toLowerCase().includes(normalizedQuery) ||
        item.source.toLowerCase().includes(normalizedQuery);
      const matchesSource = sourceFilter === "all" || item.source === sourceFilter;
      const matchesStatus = statusFilter === "all" || item.status === statusFilter;
      return matchesQuery && matchesSource && matchesStatus;
    })
    .sort((a, b) => sortLibraryItems(a, b, sortKey));
  const selectedCount = selectedItemIds.size;
  const selectedItems = items.filter((item) => selectedItemIds.has(item.id));
  const filteredItemIds = filteredItems.map((item) => item.id);
  const visibleSelectedCount = filteredItemIds.filter((itemId) => selectedItemIds.has(itemId)).length;
  const allFilteredSelected = filteredItemIds.length > 0 && visibleSelectedCount === filteredItemIds.length;
  const batchPending = batchState.status === "reindexing" || batchState.status === "deleting";
  const failedCleanupCount = failedCleanupIds.length;
  const loadedCapabilityCounts = useMemo(() => countLibraryCapabilities(items), [items]);
  const capabilityCounts = allCapabilityCounts ?? loadedCapabilityCounts;

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

  async function collectAllCapabilityCounts(): Promise<LibraryCapabilityCounts> {
    const pageSize = 1000;
    const allItems: LibraryCapabilityItem[] = [];
    for (let cursor = 0; ; cursor += pageSize) {
      const page = await api.listItems({
        limit: pageSize,
        cursor,
        includeUsage: false,
      });
      allItems.push(...page.map(libraryCapabilityItemFromRecord));
      if (page.length < pageSize) {
        break;
      }
    }
    return countLibraryCapabilities(allItems);
  }

  useEffect(() => {
    let cancelled = false;
    if (!actionsEnabled || items.length === 0) {
      setAllCapabilityCounts(null);
      return;
    }
    collectAllCapabilityCounts()
      .then((counts) => {
        if (!cancelled) {
          setAllCapabilityCounts(counts);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setAllCapabilityCounts(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [actionsEnabled, itemCapabilitySignature, jobStatusSignature]);

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
  }, [actionsEnabled, itemStatusSignature, jobStatusSignature]);

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

  return (
    <div className="page wide">
      <div className="page-head row" style={{ alignItems: "flex-end", justifyContent: "space-between" }}>
        <div>
          <h1 className="page-h1">{t("library.heading")}</h1>
          <p className="page-sub" style={{ maxWidth: 520 }}>{t("library.sub")}</p>
        </div>
        <div className="row gap-2" style={{ alignItems: "center" }}>
          <div className="segmented" aria-label={t("library.view.aria")}>
            <button
              className={viewMode === "grid" ? "active" : ""}
              type="button"
              aria-label={t("library.view.grid")}
              aria-pressed={viewMode === "grid"}
              onClick={() => setViewMode("grid")}
            >
              <Library size={15} />
              <span>{t("library.view.gridShort")}</span>
            </button>
            <button
              className={viewMode === "list" ? "active" : ""}
              type="button"
              aria-label={t("library.view.list")}
              aria-pressed={viewMode === "list"}
              onClick={() => setViewMode("list")}
            >
              <ListFilter size={15} />
              <span>{t("library.view.listShort")}</span>
            </button>
          </div>
          <button className="btn btn-primary" type="button" onClick={onAddSource}>
            <Plus size={16} />
            <span>{t("home.addSource")}</span>
          </button>
        </div>
      </div>
      <IndexingStrip
        jobs={jobs}
        items={items}
        syncingSources={syncingSources}
        stepStarts={stepStarts}
        paused={indexingPaused}
        onOpen={onOpenJobs}
      />
      <div className="row gap-2 library-filter-row" style={{ flexWrap: "wrap", alignItems: "center" }}>
        <div className="search-wrap" style={{ flex: "1 1 240px" }}>
          <Search size={17} />
          <input
            className="search-input"
            value={libraryQuery}
            placeholder={t("library.searchPlaceholder")}
            onChange={(event) => setLibraryQuery(event.currentTarget.value)}
          />
        </div>
        <select
          className="select"
          aria-label={t("library.filter.sourceAria")}
          value={sourceFilter}
          onChange={(event) => setSourceFilter(event.currentTarget.value)}
        >
          <option value="all">{t("library.filter.allSources")}</option>
          {sourceOptions.map((source) => (
            <option key={source} value={source}>
              {source}
            </option>
          ))}
        </select>
        <select
          className="select"
          aria-label={t("library.filter.statusAria")}
          value={statusFilter}
          onChange={(event) => setStatusFilter(event.currentTarget.value)}
        >
          <option value="all">{t("library.filter.allStatuses")}</option>
          <option value="indexed">{t("library.status.indexed")}</option>
          <option value="indexing">{t("library.status.indexing")}</option>
          <option value="failed">{t("library.status.failed")}</option>
        </select>
        <select
          className="select"
          aria-label={t("library.sort.aria")}
          value={sortKey}
          onChange={(event) =>
            setSortKey(event.currentTarget.value as "recent" | "longest" | "shortest" | "title")
          }
        >
          <option value="recent">{t("library.sort.recent")}</option>
          <option value="longest">{t("library.sort.longest")}</option>
          <option value="shortest">{t("library.sort.shortest")}</option>
          <option value="title">{t("library.sort.title")}</option>
        </select>
      </div>
      {items.length > 0 ? (
        <div className="library-capability-summary" aria-label={t("library.capability.summary.aria")}>
          <span className="library-capability-pill warn">
            <Mic size={13} />
            {t("library.capability.speechOnly", { count: capabilityCounts.speechOnly })}
          </span>
          <span className="library-capability-pill accent">
            <Eye size={13} />
            {t("library.capability.visual", { count: capabilityCounts.visual })}
          </span>
          <span className="library-capability-pill accent">
            <FileText size={13} />
            {t("library.capability.document", { count: capabilityCounts.document })}
          </span>
          {capabilityCounts.partial > 0 ? (
            <span className="library-capability-pill warn">
              <AlertTriangle size={13} />
              {t("library.capability.partial", { count: capabilityCounts.partial })}
            </span>
          ) : null}
        </div>
      ) : null}
      <div className="row" style={{ alignItems: "center", gap: 10, marginTop: 12 }}>
        <span className="muted">
          {t("library.summary.count", { count: filteredItems.length, total: items.length })}
        </span>
        {filtersActive ? (
          <button type="button" className="btn btn-ghost sm" onClick={clearLibraryFilters}>
            {t("common.clearFilters")}
          </button>
        ) : null}
        {filteredItems.length > 0 ? (
          <button
            type="button"
            className="btn btn-ghost sm library-select-all"
            disabled={batchPending}
            onClick={toggleAllFilteredItems}
          >
            <Check size={14} />
            <span>
              {allFilteredSelected
                ? t("library.batch.selectNone")
                : t("library.batch.selectAll")}
            </span>
          </button>
        ) : null}
        {failedCleanupCount > 0 ? (
          <button
            type="button"
            className="btn btn-ghost sm"
            disabled={batchPending || !actionsEnabled}
            onClick={() => void clearFailedItems()}
            title={t("library.clearFailed.hint")}
          >
            <Trash2 size={14} />
            <span>{t("library.clearFailed.button", { count: failedCleanupCount })}</span>
          </button>
        ) : null}
      </div>
      {batchState.message ? (
        <InlineNotice
          tone={batchState.status === "error" ? "error" : "muted"}
          message={batchState.message}
        />
      ) : null}
      {selectedCount > 0 ? (
        <div
          className="library-batch-toolbar"
          aria-label={t("library.batch.aria")}
        >
          <span className="chip accent">
            <span className="dot" />
            {t("library.batch.selected", { count: selectedCount })}
          </span>
          <span className="grow" />
          <button
            type="button"
            className="btn btn-secondary sm"
            disabled={batchPending}
            onClick={() => void copySelectedDiagnostics()}
          >
            <Copy size={15} />
            <span>{t("library.batch.copyDiagnostics")}</span>
          </button>
          <button
            type="button"
            className="btn btn-secondary sm"
            disabled={batchPending || !actionsEnabled}
            onClick={() => void runBatchAction("reindex")}
          >
            {batchState.status === "reindexing" ? <Loader2 size={15} className="spin" /> : <RefreshCcw size={15} />}
            <span>{batchState.status === "reindexing" ? t("common.reindexing") : t("common.reindex")}</span>
          </button>
          <button
            type="button"
            className="btn btn-danger sm"
            disabled={batchPending || !actionsEnabled}
            onClick={() => void runBatchAction("delete")}
          >
            {batchState.status === "deleting" ? <Loader2 size={15} className="spin" /> : <Trash2 size={15} />}
            <span>{batchState.status === "deleting" ? t("common.deleting") : t("common.delete")}</span>
          </button>
          <button
            type="button"
            className="btn btn-ghost sm"
            disabled={batchPending}
            onClick={() => setSelectedItemIds(new Set())}
          >
            {t("library.batch.clear")}
          </button>
        </div>
      ) : null}
      {items.length > 0 && filteredItems.length > 0 ? (
        <div className={viewMode === "grid" ? "lib-grid" : "tbl lib-table"}>
          {viewMode === "list" ? (
            <div className="lib-table-head" aria-hidden="true">
              <span>{t("library.col.title")}</span>
              <span>{t("library.col.source")}</span>
              <span>{t("library.col.duration")}</span>
              <span>{t("library.col.indexed")}</span>
              <span>{t("library.col.searchability")}</span>
            </div>
          ) : null}
          {filteredItems.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              viewMode={viewMode}
              selectable
              selected={selectedItemIds.has(item.id)}
              onSelect={(selected) => toggleItemSelection(item.id, selected)}
              onOpen={() => onOpenItem(item)}
            />
          ))}
        </div>
      ) : items.length === 0 ? (
        <EmptyState
          title={t("library.empty.none.title")}
          body={t("library.empty.none.body")}
          actionLabel={t("library.empty.addSource")}
          onAction={onAddSource}
        />
      ) : (
        <EmptyState
          title={t("library.empty.filtered.title")}
          body={t("library.empty.filtered.body")}
          actionLabel={t("common.clearFilters")}
          onAction={clearLibraryFilters}
        />
      )}
    </div>
  );
}
