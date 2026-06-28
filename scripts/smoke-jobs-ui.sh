#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [showJobsSheet, setShowJobsSheet]" apps/desktop/src
rg -qF 'const visibleJobs = apiStatus === "online" ? data.jobs : []' apps/desktop/src
rg -qF "const drawerJobs = visibleJobs.filter" apps/desktop/src
rg -qF "jobs={drawerJobs}" apps/desktop/src
rg -qF "onClick={() => setShowJobsSheet(true)}" apps/desktop/src
rg -qF "hasActiveJobs={visibleJobs.some(isActiveJob)}" apps/desktop/src
rg -qF "export function JobsSheet" apps/desktop/src
rg -qF "role=\"dialog\"" apps/desktop/src
rg -qF "aria-labelledby=\"jobs-title\"" apps/desktop/src
rg -qF "jobStepProgressPercent(job)" apps/desktop/src
rg -qF "jobDisplayStatus(job, t)" apps/desktop/src
rg -qF "jobItemTitle(job, items, t)" apps/desktop/src
rg -qF 'await api.updateSettings({ indexing_paused: !indexingPaused })' apps/desktop/src
rg -qF 'await api.cancelJob(job.id)' apps/desktop/src
rg -qF 'export async function cancelJob' apps/desktop/src/lib/api.ts
rg -qF '"/jobs/:id/cancel"' crates/cerul-api/src/lib.rs
rg -qF "async fn cancel_job" crates/cerul-api/src/lib.rs
rg -qF 'className="btn-icon sm job-cancel"' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF '"jobs.noneTitle": "No active jobs"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "sheet-backdrop" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF ".drawer" apps/desktop/src/styles/ui.css
rg -qF ".jobs-sheet" apps/desktop/src/styles/extensions.css
rg -qF ".jobs-filters" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-timeline" apps/desktop/src/styles/handoff.css
rg -qF ".jobs-tl-card" apps/desktop/src/styles/handoff.css
rg -qF "scripts/smoke-jobs-ui.sh" scripts/smoke.sh

echo "jobs_ui_smoke home_status_opens_sheet=enabled jobs_queue_sheet=enabled progress_and_errors=enabled pause_cancel_controls=enabled empty_state=enabled"
