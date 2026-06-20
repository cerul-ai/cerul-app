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

# ② first-index cinematic takeover (the one earned takeover moment).
rg -qF "if (firstRunActive && searchDisabled)" apps/desktop/src
rg -qF "function FirstRunIndexing(" apps/desktop/src
rg -qF "onb-illo onb-illo-source fr-illo" apps/desktop/src

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

echo "first_run_ui_smoke flag=firstRunActive indexing_takeover=scheme2 ready_banner=scheme3 stepper=3step examples=clickable resolves_on=search_or_dismiss"
