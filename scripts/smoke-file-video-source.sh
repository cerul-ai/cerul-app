#!/usr/bin/env bash
# Exercises the file_video source plugin: registers a single video file as a
# source, verifies the source resolves through the registry, and the
# DiscoveredItem points back at the file we picked.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo test -p cerul-sources file_video::tests -- --nocapture
cargo test -p cerul-sources tests::registry_resolves_all_known_plugins -- --nocapture

echo "file_video_source_smoke ok"
