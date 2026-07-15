#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Persistence: firstRunActive flag lives in the desktop UI store + localStorage.
rg -qF "firstRunActive?: boolean" apps/desktop/src/lib/uiStore.ts
rg -qF "export async function persistFirstRunActive" apps/desktop/src/lib/uiStore.ts

# Gating: set on wizard hand-off, hydrated on load, resolved on first search / dismiss.
rg -qF "const [firstRunActive, setFirstRunActive] = useState(false)" apps/desktop/src
rg -qF "void persistFirstRunActive(true)" apps/desktop/src
rg -qF "setFirstRunActive(Boolean(state.firstRunActive))" apps/desktop/src
rg -qF "function resolveFirstRun()" apps/desktop/src

# Guidance only engages with real, loaded state (PR #76 review): ② needs an
# active index, ③ needs an online core with indexed content.
rg -qF "(firstRunActive || firstRunJourneyFixture) && searchDisabled && activeJobs.length > 0 && !onlyPausedQueuedJobs" apps/desktop/src
rg -qF 'const firstRunReady = firstRunActive && apiStatus === "online" && indexedCount > 0' apps/desktop/src
# Blank submits and overlay re-runs are handled (PR #76 review, P3).
rg -qF "if (submittedQuery.trim()) {" apps/desktop/src
rg -qF "loadPersistedUiState()" apps/desktop/src/OverlayApp.tsx

# Overlay (⌥Space) searches/asks resolve the same shared flag — but only when
# they actually matched indexed content (PR #76 review, P2), never on an empty
# hit during the ② takeover.
rg -qF "void persistFirstRunActive(false)" apps/desktop/src/OverlayApp.tsx
rg -qF "if (response.results.length > 0) {" apps/desktop/src/OverlayApp.tsx
rg -qF "if (answer.citations.length > 0) {" apps/desktop/src/OverlayApp.tsx

# ② first-index stage journey with a finite copper relay.
rg -qF "if (firstRunIndexing)" apps/desktop/src
rg -qF "function FirstRunIndexing(" apps/desktop/src
rg -qF "function StageJourney(" apps/desktop/src/screens/home.tsx
rg -qF 'className="first-stage-mark"' apps/desktop/src/screens/home.tsx
rg -qF '.first-stage-journey' apps/desktop/src/styles/selected-ui.css
rg -qF '@keyframes copper-relay-line' apps/desktop/src/styles/selected-ui.css
rg -qF '@keyframes copper-relay-node' apps/desktop/src/styles/selected-ui.css

# ③ ready banner + horizontal stepper over the real home, plus example chips.
rg -qF "function FirstRunReadyHeader(" apps/desktop/src
rg -qF "function FirstRunStepper(" apps/desktop/src
rg -qF "function FirstRunExamples(" apps/desktop/src
rg -qF "onClick={() => onRunQuery(text)}" apps/desktop/src

# Copy (English catalog) — the two "now what?" valleys + the search payoff.
rg -qF '"firstRun.indexing.title": "Turning your videos into searchable memory."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"firstRun.banner.title": "Your first batch is ready"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"firstRun.steps.search": "Run your first search"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"firstRun.example.said": "what was said about pricing strategy"' apps/desktop/src/lib/i18n-catalog.ts

# Styles (reuse Aurora Glass language; scoped under home-redesign.css).
rg -qF ".fr-banner" apps/desktop/src/styles/home-redesign.css
rg -qF ".fr-stepper" apps/desktop/src/styles/home-redesign.css
rg -qF ".fr-example" apps/desktop/src/styles/home-redesign.css

# Wired into the smoke entrypoint.
rg -qF "scripts/smoke-first-run-ui.sh" scripts/smoke.sh

echo "first_run_ui_smoke flag=firstRunActive indexing_takeover=copper_relay_5stage ready_banner=scheme3 examples=clickable resolves_on=search_or_dismiss"
