#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [showJobsSheet, setShowJobsSheet]" apps/desktop/src
rg -qF 'const visibleJobs = apiStatus === "online" ? data.jobs : []' apps/desktop/src
rg -qF "const drawerJobs = visibleJobs" apps/desktop/src/App.tsx
rg -qF "jobsSheetJobs" apps/desktop/src
rg -qF 'summary={apiStatus === "online" ? data.jobSummary : null}' apps/desktop/src
rg -qF "const openJobsSheet = useCallback" apps/desktop/src/App.tsx
rg -qF "onClick={openJobsSheet}" apps/desktop/src
rg -qF "hasActiveJobs={activeJobCount > 0}" apps/desktop/src
rg -qF 'apiStatus === "online" && data.jobSummary' apps/desktop/src/App.tsx
rg -qF 'status: "completed,cancelled"' apps/desktop/src/App.tsx
rg -qF "refreshJobsSheetIfFiltered" apps/desktop/src/App.tsx
rg -qF "failed to load job summary" apps/desktop/src/App.tsx
rg -qF "jobsSheetRequestSeq.current = seq" apps/desktop/src/App.tsx
rg -qF "jobsSheetDisplayedFilterRef" apps/desktop/src/App.tsx
rg -qF "filterChanged" apps/desktop/src/App.tsx
rg -qF "setJobsSheetJobs(nextJobs)" apps/desktop/src/App.tsx
rg -qF "export function JobsSheet" apps/desktop/src
rg -qF "const totalCount = (summary?.total_jobs ?? sortedJobs.length) + syncingSources.length" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF "hasAnyJobSignal" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF "showFilterControls" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF "role=\"dialog\"" apps/desktop/src
rg -qF "aria-labelledby=\"jobs-title\"" apps/desktop/src
rg -qF "jobStepProgressPercent(job)" apps/desktop/src
rg -qF "jobDisplayStatus(job, t)" apps/desktop/src
rg -qF "jobItemTitle(job, items, t)" apps/desktop/src
rg -qF 'await api.updateSettings({ indexing_paused: !indexingPaused })' apps/desktop/src
rg -qF 'await api.cancelJob(job.id)' apps/desktop/src
rg -qF 'await api.cancelQueuedJobsBatch()' apps/desktop/src
rg -qF 'cancelQueuedJobsWithCompatibilityFallback' apps/desktop/src/App.tsx
rg -qF 'error instanceof api.ApiRequestError' apps/desktop/src/App.tsx
rg -qF 'export async function cancelJob' apps/desktop/src/lib/api.ts
rg -qF 'export async function jobSummary' apps/desktop/src/lib/api.ts
rg -qF 'export async function cancelQueuedJobsBatch' apps/desktop/src/lib/api.ts
rg -qF 'export class ApiRequestError' apps/desktop/src/lib/api.ts
rg -qF '"/jobs/summary"' crates/cerul-api/src/lib.rs
rg -qF '"/jobs/cancel-batch"' crates/cerul-api/src/lib.rs
rg -qF "visible_job_count" crates/cerul-api/src/jobs.rs
rg -qF "visible_item.status != 'deleting'" crates/cerul-api/src/lib.rs
rg -qF "i.status != 'deleting'" crates/cerul-api/src/jobs.rs
rg -qF "COALESCE(j.finished_at, j.started_at, 0) DESC" crates/cerul-api/src/lib.rs
rg -qF '"/jobs/:id/cancel"' crates/cerul-api/src/lib.rs
rg -qF "async fn cancel_job" crates/cerul-api/src/lib.rs
rg -qF "async fn cancel_jobs_batch" crates/cerul-api/src/lib.rs
rg -qF 'className="btn-icon sm job-cancel"' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'jobs.clearQueued' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF '"jobs.noneTitle": "No active jobs"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "sheet-backdrop" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF ".drawer" apps/desktop/src/styles/ui.css
rg -qF ".jobs-sheet" apps/desktop/src/styles/extensions.css
rg -qF ".jobs-filters" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-timeline" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-tl-card" apps/desktop/src/styles/handoff.css
rg -qF "scripts/smoke-jobs-ui.sh" scripts/smoke.sh

echo "jobs_ui_smoke home_status_opens_sheet=enabled jobs_queue_sheet=enabled progress_and_errors=enabled pause_cancel_controls=enabled empty_state=enabled"
