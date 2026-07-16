import type { AppData } from "./types";
import { settingBoolean } from "./settings-helpers";

export function searchIndexIsSettling(
  data: Pick<AppData, "sources" | "items" | "jobs" | "jobSummary" | "settings">,
) {
  const paused = settingBoolean(data.settings, "indexing_paused", false);
  const activeJobs = data.jobs.some(
    (job) => job.status === "running" || (!paused && job.status === "queued"),
  );
  const hiddenRefreshWork = paused
    ? (data.jobSummary?.running_search_refresh_jobs ?? 0) > 0
    : (data.jobSummary?.search_refresh_jobs ?? 0) > 0;
  const queuedSearchWork = hiddenRefreshWork || (!paused &&
    data.items.some(
      (item) =>
        item.embeddingIndexStatus === "pending" ||
        item.visualIndexStatus === "pending",
    ));

  return (
    data.sources.some((source) => source.status === "syncing") ||
    activeJobs ||
    queuedSearchWork
  );
}
