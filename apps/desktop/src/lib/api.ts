const API_BASE_URL = "http://127.0.0.1:7777";

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
  chapters: VideoUnderstandingChapter[];
  events: VideoUnderstandingEvent[];
  topics: string[];
  searchable_text: string | null;
  error: string | null;
  created_at: number | null;
  updated_at: number | null;
};

export type SearchResultRecord = {
  chunk_id: string;
  item_id: string;
  chunk_type: string;
  start_sec: number | null;
  end_sec: number | null;
  snippet: string;
  frame_path: string | null;
  score: number;
  similarity_score: number | null;
  // Optional: older backends omit these, so treat them as possibly undefined.
  item_title?: string | null;
  nearest_frame_chunk_id?: string | null;
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

export async function removeSource(id: string) {
  return fetchJson<{ status: string; id: string }>(`/sources/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export async function listItems() {
  return fetchJson<ItemRecord[]>("/items");
}

export async function listJobs() {
  return fetchJson<JobRecord[]>("/jobs");
}

export async function usageSummary() {
  return fetchJson<UsageSummary>("/usage/summary");
}

export async function usageEvents(limit = 50) {
  return fetchJson<UsageEvent[]>(`/usage/events?limit=${encodeURIComponent(String(limit))}`);
}

export async function listItemChunks(id: string) {
  return fetchJson<ChunkRecord[]>(`/items/${encodeURIComponent(id)}/chunks`);
}

export function videoSegmentUrl(chunkId: string) {
  return `${API_BASE_URL}/chunks/${encodeURIComponent(chunkId)}/video-segment`;
}

export function videoClipUrl(chunkId: string, paddingSec = 2) {
  const params = new URLSearchParams({ padding_sec: String(paddingSec) });
  return `${API_BASE_URL}/chunks/${encodeURIComponent(chunkId)}/video-clip?${params.toString()}`;
}

export function chunkFrameUrl(chunkId: string) {
  return `${API_BASE_URL}/chunks/${encodeURIComponent(chunkId)}/frame`;
}

export async function deleteItem(id: string) {
  return fetchJson<{ status: string; id: string }>(`/items/${encodeURIComponent(id)}`, {
    method: "DELETE",
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

export async function getItemUnderstanding(id: string) {
  return fetchJson<VideoUnderstandingRecord>(`/items/${encodeURIComponent(id)}/understanding`);
}

export async function analyzeItemUnderstanding(id: string) {
  return fetchJson<VideoUnderstandingRecord>(`/items/${encodeURIComponent(id)}/understanding`, {
    method: "POST",
  });
}

export async function search(q: string, limit = 20) {
  return fetchJson<SearchResultRecord[]>("/search", {
    method: "POST",
    body: JSON.stringify({ q, limit }),
  });
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

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE_URL}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });

  if (!response.ok) {
    const body = await response.text();
    throw new Error(humanizeApiError(body, response.status));
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
