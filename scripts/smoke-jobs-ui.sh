#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [showJobsSheet, setShowJobsSheet]" apps/desktop/src
rg -qF "const visibleJobs = visualFixtureMode" apps/desktop/src
rg -qF "? data.jobs" apps/desktop/src
rg -qF "jobs={visibleJobs}" apps/desktop/src
rg -qF "onClick={() => setShowJobsSheet(true)}" apps/desktop/src
rg -qF "activeJobCount > 0" apps/desktop/src
rg -qF "export function JobsSheet" apps/desktop/src
rg -qF "role=\"dialog\"" apps/desktop/src
rg -qF "aria-labelledby=\"jobs-title\"" apps/desktop/src
rg -qF "jobStepProgressPercent(job)" apps/desktop/src
rg -qF "jobDisplayStatus(job, t)" apps/desktop/src
rg -qF "jobItemTitle(job, items, t)" apps/desktop/src
rg -qF '"jobs.noneTitle": "No active jobs"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "sheet-backdrop" apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF ".drawer" apps/desktop/src/styles/ui.css
rg -qF ".jobs-sheet" apps/desktop/src/styles/extensions.css
rg -qF ".job-row" apps/desktop/src/styles/app.css
rg -qF ".job-dot" apps/desktop/src/styles/app.css
rg -qF "scripts/smoke-jobs-ui.sh" scripts/smoke.sh

echo "jobs_ui_smoke home_status_opens_sheet=enabled jobs_queue_sheet=enabled progress_and_errors=enabled empty_state=enabled"
