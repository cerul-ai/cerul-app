const INTERNAL_API_PREFIX = "/internal";

import { localApiBaseUrl } from "./desktopHost";
import { appLocaleTag } from "./i18n";

export class ApiRequestError extends Error {
  constructor(
    public readonly status: number,
    message: string,
    public readonly body: string,
  ) {
    super(message);
    this.name = "ApiRequestError";
  }
}

function coreUnreachableMessage(): string {
  return appLocaleTag() === "zh-CN"
    ? "Cerul Core 暂时无法连接。"
    : "Cerul Core is not reachable yet.";
}

export type SourceRecord = {
  id: string;
  type: string;
  config: Record<string, unknown>;
  status: string;
  last_poll_at: number | null;
  created_at: number | null;
};

export type ItemRecord = {
  id: string;
  source_id: string;
  content_type: string;
  external_id: string | null;
  title: string | null;
  duration_sec: number | null;
  raw_path: string | null;
  raw_path_exists: boolean | null;
  discovered_at?: number | null;
  indexed_at: number | null;
  status: string;
  error: string | null;
  metadata: Record<string, unknown>;
  thumbnail_chunk_id: string | null;
  usage: UsageTotals;
};

export type JobRecord = {
  id: string;
  item_id: string | null;
  job_type: string;
  status: string;
  started_at: number | null;
  finished_at: number | null;
  error: string | null;
  progress: number;
  stage: string | null;
  stage_message: string | null;
  usage: UsageTotals;
  error_info: JobErrorInfo | null;
};

export type JobStatusSummary = {
  queued_jobs: number;
  running_jobs: number;
  failed_jobs: number;
  attention_jobs: number;
  indexed_items: number;
  completed_jobs: number;
  cancelled_jobs: number;
  total_jobs: number;
};

export type JobErrorInfo = {
  code: string;
  capability: string;
  settings_section: string;
  message: string;
};

export type UsageTotals = {
  event_count: number;
  request_count: number;
  input_tokens: number;
  output_tokens: number;
  audio_seconds: number;
  image_count: number;
  video_seconds: number;
  estimated_usd: number;
  billed_credits: number;
  unpriced_events: number;
};

export type UsageBreakdown = {
  key: string;
  totals: UsageTotals;
};

export type UsageSummary = {
  total: UsageTotals;
  remote: UsageTotals;
  local: UsageTotals;
  by_capability: UsageBreakdown[];
};

export type IndexingDiagnostics = {
  paused: boolean;
  configured_concurrent_jobs: number;
  effective_concurrent_jobs: number;
  effective_inference_mode: string;
  local_model_slots: number | null;
  counts: {
    total_items: number;
    indexed_items: number;
    discovered_items: number;
    processing_items: number;
    failed_items: number;
    queued_jobs: number;
    running_jobs: number;
    failed_jobs: number;
    completed_jobs: number;
  };
  active_stage_counts: { stage: string; count: number }[];
  waiting_model_jobs: number;
  active_jobs: Array<{
    id: string;
    item_id: string | null;
    job_type: string;
    stage: string | null;
    stage_message: string | null;
    progress: number;
    started_at: number | null;
  }>;
  vector_index: {
    ready: boolean;
    collection: string | null;
    point_count: number | null;
    error: string | null;
  };
};

export type MomentRecord = {
  id: string;
  item_id: string;
  chunk_id: string | null;
  start_sec: number | null;
  end_sec: number | null;
  timestamp: string;
  title: string;
  quote: string;
  note: string | null;
  created_at: number | null;
};

export type CreateMomentRequest = {
  item_id: string;
  chunk_id?: string | null;
  start_sec?: number | null;
  end_sec?: number | null;
  title?: string | null;
  quote: string;
  note?: string | null;
};

export type AskCitation = {
  playback_chunk_id: string;
  item_id: string;
  title: string;
  timestamp: string;
  start_sec: number | null;
  snippet: string;
};

export type AskUsage = {
  billable: boolean;
  credits_used: number;
  privacy: string;
};

export type AskResponse = {
  answer: string;
  citations: AskCitation[];
  usage?: AskUsage;
};

export type AgentToolContract = {
  name: string;
  description: string;
  method: string;
  path: string;
  stage: string;
  input_schema: Record<string, unknown>;
  output_contract: string;
  safety: {
    read_only: boolean;
    billable: boolean;
    requires_confirmation: boolean;
    arbitrary_shell: boolean;
    arbitrary_file_write: boolean;
  };
  evidence: {
    returns_evidence_locators: boolean;
    opens_in_cerul: boolean;
  };
};

export type AgentToolsResponse = {
  request_id: string;
  execution: {
    target: string;
    account_id: string | null;
    privacy: string;
  };
  runtime: {
    tool_host: string;
    renderer_access: string;
    arbitrary_shell: boolean;
    arbitrary_file_write: boolean;
    write_actions_require_confirmation: boolean;
  };
  tools: AgentToolContract[];
};

type V1AskResponse = {
  execution: {
    privacy: string;
  };
  answer: string;
  citations: V1SearchResult[];
  usage: {
    billable: boolean;
    credits_used: number;
  };
};

type V1SearchResult = {
  id: string;
  item: {
    id: string;
    title: string;
    content_type: string;
    source_type: string;
  };
  time: {
    start_sec: number | null;
    timestamp: string | null;
  };
  text: {
    snippet: string;
    quote: string;
  };
};

export type EntitySummary = {
  id: string;
  label: string;
  kind: string;
  mention_count: number;
  item_count: number;
};

export type EntityMention = {
  entity_id: string;
  label: string;
  kind: string;
  item_id: string;
  item_title: string;
  chunk_id: string | null;
  timestamp: string;
  start_sec: number | null;
  quote: string;
};

export type EntityDetail = {
  entity: EntitySummary;
  mentions: EntityMention[];
};

export type WeeklyTopic = {
  id: string;
  label: string;
  count: number;
};

export type WeeklyReview = {
  week_start: number;
  indexed_items: number;
  indexed_seconds: number;
  watched_percent: number;
  topics: WeeklyTopic[];
  has_data: boolean;
};

export type PlaybackPositionRecord = {
  item_id: string;
  position_sec: number;
  timestamp: string;
  chunk_id: string | null;
  updated_at: number | null;
};

export type StorageUsageCategory = {
  key: string;
  label: string;
  bytes: number;
  apparent_bytes: number;
};

export type StorageUsageResponse = {
  data_dir: string;
  total_bytes: number;
  total_apparent_bytes: number;
  categories: StorageUsageCategory[];
};

export type UsageEvent = {
  id: string;
  created_at: number | null;
  provider_mode: "remote" | "local" | "byok" | "cloud" | "self_host";
  capability: string;
  provider_id: string | null;
  provider_type: string | null;
  model_id: string | null;
  item_id: string | null;
  job_id: string | null;
  status: string;
  request_count: number;
  input_tokens: number | null;
  output_tokens: number | null;
  audio_seconds: number | null;
  image_count: number | null;
  video_seconds: number | null;
  estimated_usd: number | null;
  billed_credits: number | null;
  price_snapshot_id: string | null;
  metadata: Record<string, unknown>;
};

export type ChunkRecord = {
  id: string;
  item_id: string;
  chunk_type: string;
  start_sec: number | null;
  end_sec: number | null;
  text: string | null;
  frame_path: string | null;
  metadata: Record<string, unknown>;
};

export type VideoUnderstandingChapter = {
  start_sec: number | null;
  end_sec: number | null;
  title: string;
  summary: string;
};

export type VideoUnderstandingEvent = {
  start_sec: number | null;
  end_sec: number | null;
  caption: string;
  visual: string | null;
  audio: string | null;
  actions: string[];
  entities: string[];
  confidence: number | null;
};

export type VideoUnderstandingRecord = {
  item_id: string;
  provider_id: string | null;
  model_id: string | null;
  status: "not_started" | "running" | "completed" | "failed";
  summary: string | null;
  // A short human-readable title produced by the video understanding pass. It
  // is also written back to items.metadata.display_title by Cerul Core so
  // library cards and detail pages can use it as the primary content title.
  display_title?: string | null;
  chapters: VideoUnderstandingChapter[];
  events: VideoUnderstandingEvent[];
  topics: string[];
  searchable_text: string | null;
  error: string | null;
  created_at: number | null;
  updated_at: number | null;
};

export type SearchResultRecord = {
  playback_chunk_id?: string | null;
  // Older local cores returned the selected chunk as chunk_id. Keep accepting it
  // so a freshly updated desktop can still search against a stale core process.
  chunk_id?: string | null;
  item_id: string;
  chunk_type: string;
  start_sec: number | null;
  end_sec: number | null;
  snippet: string;
  frame_path: string | null;
  // Optional while older local cores are still possible during development.
  match_score?: number | null;
  score: number;
  similarity_score: number | null;
  exact_match?: boolean;
  // Optional: older backends omit these, so treat them as possibly undefined.
  item_title?: string | null;
  nearest_frame_chunk_id?: string | null;
};

export type SearchRankingPreference = "smart" | "video" | "image" | "document" | "audio";

export type SearchOptions = {
  rankingPreference?: SearchRankingPreference;
};

export type SearchDiagnostics = {
  retrieval_mode:
    | "unified_vector"
    | "hybrid"
    | "vector"
    | "fts"
    | "fts_fallback"
    | "empty"
    | string;
  fallback_reason: string | null;
  vector_hits_count: number;
  text_vector_hits_count: number;
  image_vector_hits_count: number;
  fts_hits_count: number;
  embedding_profile_id: string | null;
  vector_index_collection: string | null;
  vector_index_point_count: number | null;
  vector_index_text_collection: string | null;
  vector_index_image_collection: string | null;
  vector_index_text_points: number | null;
  vector_index_image_points: number | null;
  retrieval_unit_count?: number;
  indexed_item_count?: number;
  items_needing_rebuild?: number;
};

export type SearchResponseRecord = {
  results: SearchResultRecord[];
  diagnostics: SearchDiagnostics;
};

export type SearchHealthDiagnostics = {
  item_count: number;
  indexed_item_count: number;
  search_index_version: number;
  retrieval_unit_count: number;
  unified_indexed_item_count: number;
  items_needing_rebuild: number;
  chunk_count: number;
  searchable_text_chunk_count: number;
  image_chunk_count: number;
  fts_row_count: number;
  retrieval_unit_fts_row_count: number;
  orphan_job_count: number;
  missing_raw_path_count: number;
  embedding_profile_id: string | null;
  vector_index_collection: string | null;
  vector_index_point_count: number | null;
  vector_index_expected_point_count: number;
  vector_index_drift_item_count: number;
  vector_index_text_collection: string | null;
  vector_index_image_collection: string | null;
  vector_index_text_points: number | null;
  vector_index_image_points: number | null;
  embedded_text_chunk_count: number | null;
  embedded_image_chunk_count: number | null;
  text_embedding_gap_count: number | null;
  image_embedding_gap_count: number | null;
  vector_index_error: string | null;
};

export type SearchRebuildResponse = {
  rebuild_queued_items: number;
  queued_jobs: number;
  diagnostics: SearchHealthDiagnostics;
};

export type DiagnosticsBundle = {
  generated_at: number;
  app_version: string;
  runtime: Record<string, unknown>;
  settings: SettingsMap;
  local_models: LocalPrepareStatus | null;
  local_models_error: string | null;
  search: SearchHealthDiagnostics;
  jobs: Record<string, unknown>[];
  recent_errors: Record<string, unknown>[];
};

export type SettingsMap = Record<string, unknown>;

export type WhisperModelRecord = {
  id: string;
  label: string;
  filename: string;
  size_bytes: number;
  size_label: string;
  url: string;
  installed: boolean;
  selected: boolean;
  path: string | null;
};

export type ModelDownloadResponse = {
  id: string;
  installed: boolean;
  path: string;
  size_bytes: number;
};

export type EmbeddingProfile = {
  id: string;
  provider_id: string;
  model_id: string;
  model_revision: string | null;
  output_dimension: number;
  distance_metric: string;
  instruction_template: string | null;
  index_version: number;
  status: string;
};

export type ModelRuntimeStatus = {
  platform: string;
  api_runtime_ready: boolean;
  local_runtime_ready: boolean;
  openai_ready: boolean;
  gemini_ready: boolean;
  last_error: string | null;
  local_runtime_error: string | null;
};

export type ModelCatalogRecord = {
  id: string;
  label: string;
  capability: string;
  tier: string;
  format: string;
  source: string;
  size_label: string;
  install_behavior: string;
  required_for_first_search: boolean;
  recommended: boolean;
  installed: boolean;
  selected: boolean;
  blocked_reason: string | null;
};

export type ModelCatalogResponse = {
  models: ModelCatalogRecord[];
  active_embedding_profile: EmbeddingProfile;
  embedding_profiles: EmbeddingProfile[];
  runtime: ModelRuntimeStatus;
};

export type ProviderType =
  | "local"
  | "openai"
  | "anthropic"
  | "gemini"
  | "openai-compatible";

export type ProviderRecord = {
  id: string;
  type: ProviderType;
  label: string;
  base_url: string | null;
  status: "ready" | "unconfigured" | "error";
  last_error: string | null;
  has_key: boolean;
  key_preview: string | null;
  created_at: number | null;
  updated_at: number | null;
};

export type ProviderModelRecord = {
  id: string;
  label: string;
  source: string;
};

export type CreateProviderRequest = {
  type: Exclude<ProviderType, "local">;
  label: string;
  base_url?: string;
  api_key?: string;
};

export type UpdateProviderRequest = {
  type?: Exclude<ProviderType, "local">;
  label?: string;
  base_url?: string;
  api_key?: string;
};

export type RssSourcePreview = {
  feed_url: string;
  title: string;
  image_url: string | null;
  episode_count: number;
};

export async function health() {
  return fetchJson<{ status: string; version: string }>("/health");
}

export async function listSources() {
  return fetchJson<SourceRecord[]>("/sources");
}

export async function addSource(type: string, config: Record<string, unknown>) {
  return fetchJson<SourceRecord>("/sources", {
    method: "POST",
    body: JSON.stringify({ type, config }),
  });
}

export async function previewRssSource(url: string) {
  return fetchJson<RssSourcePreview>("/sources/preview/rss", {
    method: "POST",
    body: JSON.stringify({ url }),
  });
}

export async function pauseSource(id: string) {
  return fetchJson<{ status: string; id: string }>(`/sources/${encodeURIComponent(id)}/pause`, {
    method: "POST",
  });
}

export async function resumeSource(id: string) {
  return fetchJson<{ status: string; id: string }>(`/sources/${encodeURIComponent(id)}/resume`, {
    method: "POST",
  });
}

export async function retryFailedSourceItems(id: string) {
  return fetchJson<{ status: string; id: string; items: number; queued_jobs: number }>(
    `/sources/${encodeURIComponent(id)}/retry-failed`,
    { method: "POST" },
  );
}

export async function retrySourceDiscovery(id: string) {
  return fetchJson<{ status: string; id: string }>(
    `/sources/${encodeURIComponent(id)}/retry-discovery`,
    { method: "POST" },
  );
}

export async function removeSource(id: string) {
  return fetchJson<{ status: string; id: string }>(`/sources/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export async function listItems(params?: {
  status?: string;
  sourceId?: string;
  limit?: number;
  cursor?: number;
  light?: boolean;
  includeUsage?: boolean;
}) {
  const qs = new URLSearchParams();
  if (params?.status) qs.set("status", params.status);
  if (params?.sourceId) qs.set("source_id", params.sourceId);
  if (params?.limit != null) qs.set("limit", String(params.limit));
  if (params?.cursor != null) qs.set("cursor", String(params.cursor));
  if (params?.light) qs.set("light", "true");
  if (params?.includeUsage != null) qs.set("include_usage", params.includeUsage ? "true" : "false");
  const suffix = qs.toString();
  return fetchJson<ItemRecord[]>(`/items${suffix ? `?${suffix}` : ""}`);
}

export type ListJobsParams = {
  status?: string;
  scope?: string;
  limit?: number;
  cursor?: number;
  light?: boolean;
  includeUsage?: boolean;
};

export async function listJobs(params?: ListJobsParams) {
  const qs = new URLSearchParams();
  if (params?.status) qs.set("status", params.status);
  if (params?.scope) qs.set("scope", params.scope);
  if (params?.limit != null) qs.set("limit", String(params.limit));
  if (params?.cursor != null) qs.set("cursor", String(params.cursor));
  if (params?.light) qs.set("light", "true");
  if (params?.includeUsage != null) qs.set("include_usage", params.includeUsage ? "true" : "false");
  if (!params?.status && !params?.scope) qs.set("scope", "drawer");
  const suffix = qs.toString();
  return fetchJson<JobRecord[]>(`/jobs${suffix ? `?${suffix}` : ""}`);
}

export async function jobSummary() {
  return fetchJson<JobStatusSummary>("/jobs/summary");
}

export async function cancelJob(id: string) {
  return fetchJson<{ status: string; id: string; item_id: string | null }>(
    `/jobs/${encodeURIComponent(id)}/cancel`,
    { method: "POST" },
  );
}

export async function cancelQueuedJobsBatch(params?: { ids?: string[]; sourceId?: string }) {
  return fetchJson<{ status: string; cancelled: number; ids: string[]; item_ids: string[] }>(
    "/jobs/cancel-batch",
    {
      method: "POST",
      body: JSON.stringify({
        ids: params?.ids,
        status: params?.ids?.length ? undefined : "queued",
        source_id: params?.sourceId,
      }),
    },
  );
}

export async function listMoments() {
  return fetchJson<MomentRecord[]>("/moments");
}

export async function createMoment(request: CreateMomentRequest) {
  return fetchJson<MomentRecord>("/moments", {
    method: "POST",
    body: JSON.stringify(request),
  });
}

export async function deleteMoment(id: string) {
  return fetchJson<{ status: string; id: string }>(`/moments/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export async function askLibrary(q: string, limit = 6, locale?: string) {
  return fetchJson<AskResponse>("/ask", {
    method: "POST",
    body: JSON.stringify({ q, limit, locale }),
  });
}

export function isAgentExperienceEnabled() {
  return ["1", "true", "yes"].includes(
    String(import.meta.env.VITE_CERUL_AGENT ?? "").toLowerCase(),
  );
}

export async function listAgentTools() {
  return fetchV1Json<AgentToolsResponse>("/agent/tools");
}

export async function askAgentLibrary(q: string, limit = 6, locale?: string) {
  const response = await fetchV1Json<V1AskResponse>("/ask", {
    method: "POST",
    body: JSON.stringify({ question: q, max_results: limit, locale, target: "local" }),
  });

  return {
    answer: response.answer,
    citations: response.citations.map(v1SearchResultToAskCitation),
    usage: {
      billable: response.usage.billable,
      credits_used: response.usage.credits_used,
      privacy: response.execution.privacy,
    },
  } satisfies AskResponse;
}

function v1SearchResultToAskCitation(result: V1SearchResult): AskCitation {
  const timestamp =
    result.time.timestamp ??
    (result.item.content_type === "document" ? "Document" : "00:00");
  return {
    playback_chunk_id: result.id,
    item_id: result.item.id,
    title: result.item.title,
    timestamp,
    start_sec: result.time.start_sec,
    snippet: result.text.snippet || result.text.quote,
  };
}

export async function listEntities() {
  return fetchJson<EntitySummary[]>("/entities");
}

export async function getEntity(id: string) {
  return fetchJson<EntityDetail>(`/entities/${encodeURIComponent(id)}`);
}

export async function weeklyReview() {
  return fetchJson<WeeklyReview>("/weekly-review");
}

export async function usageSummary() {
  return fetchJson<UsageSummary>("/usage/summary");
}

export async function indexingDiagnostics() {
  return fetchJson<IndexingDiagnostics>("/diagnostics/indexing");
}

export async function storageUsage() {
  return fetchJson<StorageUsageResponse>("/storage/usage");
}

export async function usageEvents(limit = 50) {
  return fetchJson<UsageEvent[]>(`/usage/events?limit=${encodeURIComponent(String(limit))}`);
}

export async function listItemChunks(id: string) {
  return fetchJson<ChunkRecord[]>(`/items/${encodeURIComponent(id)}/chunks`);
}

export function videoSegmentUrl(chunkId: string) {
  return mediaSegmentUrl(chunkId);
}

export function mediaSegmentUrl(chunkId: string) {
  return `${localApiBaseUrl()}${INTERNAL_API_PREFIX}/chunks/${encodeURIComponent(chunkId)}/video-segment`;
}

export function videoClipUrl(
  chunkId: string,
  opts: { beforeSec?: number; afterSec?: number; paddingSec?: number } = {},
) {
  const params = new URLSearchParams();
  if (opts.beforeSec !== undefined) params.set("before_sec", String(opts.beforeSec));
  if (opts.afterSec !== undefined) params.set("after_sec", String(opts.afterSec));
  params.set("padding_sec", String(opts.paddingSec ?? 2));
  return `${localApiBaseUrl()}${INTERNAL_API_PREFIX}/chunks/${encodeURIComponent(chunkId)}/video-clip?${params.toString()}`;
}

export function chunkFrameUrl(chunkId: string) {
  return `${localApiBaseUrl()}${INTERNAL_API_PREFIX}/chunks/${encodeURIComponent(chunkId)}/frame`;
}

export async function deleteItem(id: string, options?: { keepDiscoverable?: boolean }) {
  // keepDiscoverable skips the backend ignored-item tombstone so the item can be
  // re-discovered/re-imported (used by the library "clear failed" cleanup).
  const query = options?.keepDiscoverable ? "?keep_discoverable=true" : "";
  return fetchJson<{ status: string; id: string }>(
    `/items/${encodeURIComponent(id)}${query}`,
    { method: "DELETE" },
  );
}

export async function updateItemRawPath(id: string, rawPath: string) {
  return fetchJson<ItemRecord>(`/items/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ raw_path: rawPath }),
  });
}

export async function reindexItem(id: string) {
  return fetchJson<{ status: string; id: string; queued_job: boolean }>(
    `/items/${encodeURIComponent(id)}/reindex`,
    {
      method: "POST",
    },
  );
}

export async function updatePlaybackPosition(
  id: string,
  positionSec: number,
  chunkId?: string | null,
) {
  return fetchJson<PlaybackPositionRecord>(`/items/${encodeURIComponent(id)}/playback`, {
    method: "PATCH",
    body: JSON.stringify({
      position_sec: positionSec,
      chunk_id: chunkId ?? null,
    }),
  });
}

export async function getItemUnderstanding(id: string) {
  return fetchJson<VideoUnderstandingRecord>(`/items/${encodeURIComponent(id)}/understanding`);
}

export async function analyzeItemUnderstanding(id: string) {
  return fetchJson<VideoUnderstandingRecord>(`/items/${encodeURIComponent(id)}/understanding`, {
    method: "POST",
  });
}

export async function search(q: string, limit = 20, options: SearchOptions = {}) {
  return fetchJson<SearchResponseRecord>("/search", {
    method: "POST",
    body: JSON.stringify({
      q,
      limit,
      rankingPreference: options.rankingPreference ?? "smart",
    }),
  });
}

export async function searchDiagnostics() {
  return fetchJson<SearchHealthDiagnostics>("/search/diagnostics");
}

export async function rebuildSearchIndex() {
  return fetchJson<SearchRebuildResponse>("/search/rebuild", {
    method: "POST",
  });
}

export async function diagnosticsBundle() {
  return fetchJson<DiagnosticsBundle>("/diagnostics");
}

export async function listWhisperModels() {
  return fetchJson<WhisperModelRecord[]>("/models/whisper");
}

export async function getModelCatalog() {
  return fetchJson<ModelCatalogResponse>("/models/catalog");
}

export async function downloadWhisperModel(id: string) {
  return fetchJson<ModelDownloadResponse>(`/models/whisper/${encodeURIComponent(id)}/download`, {
    method: "POST",
  });
}

export interface AutoDownloadStatus {
  in_progress: boolean;
  model_id: string;
  size_label: string;
  last_error: string | null;
  any_model_installed: boolean;
  downloaded_bytes: number;
  total_bytes: number;
  bytes_per_second: number;
  eta_seconds: number;
}

export async function getAutoDownloadStatus() {
  return fetchJson<AutoDownloadStatus>("/models/whisper/auto-download-status");
}

export interface EmbeddingStatus {
  ready: boolean;
  preparing: boolean;
  cached_mb: number;
  last_error: string | null;
  download_source: string;
  download_proxy_configured: boolean;
}

export async function getEmbeddingStatus() {
  return fetchJson<EmbeddingStatus>("/models/embed/status");
}

export async function prepareEmbeddingModels() {
  return fetchJson<EmbeddingStatus>("/models/embed/prepare", {
    method: "POST",
  });
}

export async function listProviders() {
  return fetchJson<ProviderRecord[]>("/providers");
}

export async function createProvider(request: CreateProviderRequest) {
  return fetchJson<ProviderRecord>("/providers", {
    method: "POST",
    body: JSON.stringify(request),
  });
}

export async function updateProvider(id: string, request: UpdateProviderRequest) {
  return fetchJson<ProviderRecord>(`/providers/${encodeURIComponent(id)}`, {
    method: "PATCH",
    body: JSON.stringify(request),
  });
}

export async function deleteProvider(id: string) {
  return fetchJson<{ status: string; id: string }>(`/providers/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export async function testProvider(id: string) {
  return fetchJson<ProviderRecord>(`/providers/${encodeURIComponent(id)}/test`, {
    method: "POST",
  });
}

export async function discoverProviderModels(id: string) {
  return fetchJson<ProviderModelRecord[]>(`/providers/${encodeURIComponent(id)}/models`);
}

export async function listSettings() {
  return fetchJson<SettingsMap>("/settings");
}

export async function updateSettings(settings: SettingsMap) {
  return fetchJson<SettingsMap>("/settings", {
    method: "PATCH",
    body: JSON.stringify(settings),
  });
}

// ---- Local on-device models: machine capability + download prepare ----
export type LocalModelInfo = {
  id: string;
  label: string;
  size_mb: number;
  status: "pending" | "downloading" | "ready";
  progress: number; // 0–100
};

export type LocalModelCapability = {
  can_run_local: boolean;
  apple_silicon: boolean;
  arch: string;
  ram_gb: number;
  recommended: "local" | "remote";
  total_mb: number;
  models: { id: string; label: string; size_mb: number }[];
};

export type LocalPrepareStatus = {
  phase: "idle" | "downloading" | "ready" | "error";
  overall_progress: number; // 0–100
  done_mb: number;
  total_mb: number;
  eta_seconds: number | null;
  active_source: string | null;
  source_label: string | null;
  download_bps: number | null;
  can_pause: boolean;
  can_cancel: boolean;
  last_source_error: string | null;
  last_source: string | null;
  last_source_label: string | null;
  last_download_bps: number | null;
  probes: ProbeResult[] | null;
  models: LocalModelInfo[];
  error: string | null;
};

export type ProbeResult = {
  source: string;
  ok: boolean;
  bytes_per_second: number;
  ttfb_ms: number | null;
  bytes: number;
  error?: string;
};

export async function localModelCapability() {
  return fetchJson<LocalModelCapability>("/models/local/capability");
}

export async function prepareLocalModels(modelIds?: string[]) {
  return fetchJson<LocalPrepareStatus>("/models/local/prepare", {
    method: "POST",
    body: JSON.stringify({ models: modelIds ?? null }),
  });
}

export async function localPrepareStatus() {
  return fetchJson<LocalPrepareStatus>("/models/local/prepare-status");
}

export async function cancelLocalModelPrepare() {
  return fetchJson<LocalPrepareStatus>("/models/local/prepare-cancel", {
    method: "POST",
  });
}

export async function deleteLocalModels(modelIds?: string[]) {
  return fetchJson<LocalPrepareStatus>("/models/local/delete", {
    method: "POST",
    body: JSON.stringify({ models: modelIds ?? null }),
  });
}

export async function repairLocalModels(modelIds?: string[]) {
  return fetchJson<LocalPrepareStatus>("/models/local/repair", {
    method: "POST",
    body: JSON.stringify({ models: modelIds ?? null }),
  });
}

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${localApiBaseUrl()}${INTERNAL_API_PREFIX}${path}`, {
      ...init,
      headers: {
        "Content-Type": "application/json",
        ...init?.headers,
      },
    });
  } catch {
    // Network-level failure (core not running / connection refused). The
    // browser's raw "Failed to fetch" is neither localized nor actionable.
    throw new Error(coreUnreachableMessage());
  }

  if (!response.ok) {
    const body = await response.text();
    throw new ApiRequestError(response.status, humanizeApiError(body, response.status), body);
  }

  return response.json() as Promise<T>;
}

async function fetchV1Json<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${localApiBaseUrl()}/v1${path}`, {
      ...init,
      headers: {
        "Content-Type": "application/json",
        ...init?.headers,
      },
    });
  } catch {
    throw new Error(coreUnreachableMessage());
  }

  if (!response.ok) {
    const body = await response.text();
    throw new ApiRequestError(response.status, humanizeApiError(body, response.status), body);
  }

  return response.json() as Promise<T>;
}

// Cerul Core returns errors as JSON envelopes like {"error":"item not found"}.
// Surfacing the raw body in the UI looks broken, so unwrap it into a sentence.
function humanizeApiError(body: string, status: number): string {
  const fallback = `Cerul Core returned ${status}`;
  const trimmed = body.trim();
  if (!trimmed) {
    return fallback;
  }
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed) as Record<string, unknown>;
      const field = ["error", "message", "detail"]
        .map((key) => parsed?.[key])
        .find((value): value is string => typeof value === "string" && value.trim().length > 0);
      return field ? sentenceCase(field) : fallback;
    } catch {
      // Not valid JSON after all — fall through to the raw text.
    }
  }
  return sentenceCase(trimmed);
}

function sentenceCase(value: string): string {
  const text = value.trim();
  if (!text) {
    return text;
  }
  return text.charAt(0).toUpperCase() + text.slice(1);
}
