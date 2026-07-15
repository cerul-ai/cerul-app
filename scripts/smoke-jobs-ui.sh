#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'const visibleJobs = apiStatus === "online" ? data.jobs : []' apps/desktop/src
rg -qF "const drawerJobs = visibleJobs" apps/desktop/src/App.tsx
rg -qF "jobsSheetJobs" apps/desktop/src
rg -qF 'summary={apiStatus === "online" ? jobsSheetSummaryWithFixture : null}' apps/desktop/src
rg -qF "function openJobsSheet()" apps/desktop/src/App.tsx
rg -qF 'navigate("jobs")' apps/desktop/src/App.tsx
rg -qF 'view === "jobs"' apps/desktop/src/App.tsx
rg -qF 'embedded' apps/desktop/src/App.tsx
rg -qF "onClick={openJobsSheet}" apps/desktop/src
rg -qF "hasActiveJobs={activeJobCount > 0}" apps/desktop/src
rg -qF 'apiStatus === "online" && data.jobSummary' apps/desktop/src/App.tsx
rg -qF 'const taskAttentionCount = apiStatus === "online" && data.jobSummary' apps/desktop/src/App.tsx
rg -qF 'data.jobSummary.attention_jobs' apps/desktop/src/App.tsx
rg -qF 'attention_jobs: attention_job_count(&conn)?' crates/cerul-api/src/jobs.rs
rg -qF "active.status IN ('queued', 'running')" crates/cerul-api/src/jobs.rs
rg -qF 'indexedItemCount={indexedItemCount}' apps/desktop/src/App.tsx
rg -qF 'taskAttentionCount={taskAttentionCount}' apps/desktop/src/App.tsx
rg -qF 'className="badge-count task-attention-count"' apps/desktop/src/components/bridge.tsx
rg -qF 'status: "completed,cancelled"' apps/desktop/src/App.tsx
rg -qF "refreshJobsSheetIfFiltered" apps/desktop/src/App.tsx
rg -qF "failed to load job summary" apps/desktop/src/App.tsx
rg -qF "jobsSheetRequestSeq.current = seq" apps/desktop/src/App.tsx
rg -qF "jobsSheetDisplayedFilterRef" apps/desktop/src/App.tsx
rg -qF "filterChanged" apps/desktop/src/App.tsx
rg -qF "setJobsSheetJobs(nextJobs)" apps/desktop/src/App.tsx
rg -qF "export function JobsSheet" apps/desktop/src
rg -qF "const totalCount = (summary?.total_jobs ?? sortedJobs.length) + syncingSources.length" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'className={embedded ? "jobs-ledger-dialog jobs-sheet is-page" : "jobs-ledger-dialog jobs-sheet"}' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'className={issueOpen && repairJob ? "jobs-ledger-workspace has-issue" : "jobs-ledger-workspace"}' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'className={`jobs-repair-cabin phase-${repairPhase}`}' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'const repairJobCanRetry = Boolean(repairJob?.item_id)' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'repairJobCanRetry ? <button' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'view === "shares" || view === "jobs"' apps/desktop/src/App.tsx
rg -qF 'const jobsReturnRouteRef = useRef<' apps/desktop/src/App.tsx
rg -qF 'hash: window.location.hash || "#home"' apps/desktop/src/App.tsx
rg -qF 'const returnRoute = jobsReturnRouteRef.current' apps/desktop/src/App.tsx
rg -qF 'await onRetryJob(job)' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'if (from && returnRowRef.current) await flyTransfer(from, returnRowRef.current, job)' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'onCancelJobs={async (selectedJobs) =>' apps/desktop/src/App.tsx
rg -qF 'await Promise.all(selectedJobs.map((job) => api.cancelJob(job.id)))' apps/desktop/src/App.tsx
rg -qF 'await api.reindexItem(job.item_id)' apps/desktop/src/App.tsx
rg -qF 'className="bridge-jobs-popover"' apps/desktop/src/components/bridge.tsx
rg -qF 'jobs.popover.viewAll' apps/desktop/src/components/bridge.tsx
rg -qF 'role={embedded ? "region" : "dialog"}' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF "aria-labelledby=\"jobs-ledger-title\"" apps/desktop/src
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
rg -qF 'className="jobs-ledger-action"' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'jobs.clearQueued' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF '"jobs.noneTitle": "No active jobs"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "sheet-backdrop" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF ".drawer" apps/desktop/src/styles/ui.css
rg -qF ".jobs-sheet" apps/desktop/src/styles/extensions.css
rg -qF ".jobs-filters" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-timeline" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-tl-card" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-ledger-workspace.has-issue" apps/desktop/src/styles/selected-ui.css
rg -qF ".jobs-transfer-ghost" apps/desktop/src/styles/selected-ui.css
rg -qF 'className="jobs-current-inspector"' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'useEscapeToClose(onClose, true)' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'className="jobs-ledger-back"' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'className="jobs-activity-event is-current"' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'className={`jobs-ledger-row${inspectedJob?.id === job.id ? " is-inspected" : ""}' apps/desktop/src/dialogs/jobs-sheet.tsx
! rg -qF 't("jobs.inspector.location")' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF '.jobs-ledger-row.is-inspected::before' apps/desktop/src/styles/selected-ui.css
rg -qF '.jobs-activity-event' apps/desktop/src/styles/selected-ui.css
rg -qF 'className={item?.thumbnailUrl ? "jobs-ledger-thumb has-image" : "jobs-ledger-thumb"}' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'const nextIssue = filteredJobs.find' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'if (activeIssueJob && jobGroup(activeIssueJob) === "failed") return;' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'const selectedJobs = pageJobs.filter' apps/desktop/src/dialogs/jobs-sheet.tsx
! rg -qF 'const selectedJobs = ledgerJobs.filter' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'const queuedJobIds = jobsSheetVisibleJobs' apps/desktop/src/App.tsx
rg -qF 'grid-template-columns:0 minmax(0,1fr) 340px' apps/desktop/src/styles/selected-ui.css
rg -qF '.jobs-ledger-page {' apps/desktop/src/styles/selected-ui.css
rg -qF '.jobs-ledger-dialog.jobs-sheet.is-page {' apps/desktop/src/styles/selected-ui.css
rg -qF "scripts/smoke-jobs-ui.sh" scripts/smoke.sh

echo "jobs_ui_smoke bridge_glance=T1_unresolved_attention_only route_page=enabled esc_back=enabled title_left_back=enabled selection=settings_sweep timeline=vertical location=removed columns=safe layout=J6_ledger_plus_current_inspector anomaly=H2_priority_cabin repair_roundtrip=A_entity_loop"
