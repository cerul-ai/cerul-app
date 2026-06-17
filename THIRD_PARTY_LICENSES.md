# Third-Party Licenses

This file records release-relevant third-party components. It is not a complete
transitive dependency report yet; public release candidates must attach a full
generated dependency/license inventory.

## Source Dependencies

- Rust crates are resolved through `Cargo.lock`.
- JavaScript packages are resolved through `pnpm-lock.yaml`.
- Python runtime experiments live under `mlx-sidecar/` and are not bundled as a
  complete runtime by default.

Before a public release, generate and review:

```bash
cargo metadata --format-version 1
cargo audit
cargo deny check advisories licenses sources
pnpm licenses list --prod
```

The Rust dependency license/source gate is encoded in `deny.toml` and runs in
CI. This file still records release-relevant packaged binaries and runtime
assets that generated language-package reports cannot fully validate.

## Packaged Binaries

Release builds may copy these generated artifacts into the app bundle:

| Component | Source | License posture | Release gate |
|---|---|---|---|
| `ffmpeg` | staged by `scripts/fetch-binaries.sh` | Must be LGPL-compatible for commercial distribution | Confirm build flags do not enable GPL components such as x264/x265 or other `--enable-gpl` features. |
| `yt-dlp` | official GitHub releases | Unlicense | Keep an update path because site extractors become stale. |
| `qdrant` | official Qdrant releases | Apache-2.0 | Verify platform artifact and bundled path before installer release. |
| `cerul-api` | built from this repository | FSL-1.1-ALv2 | Built and staged by `apps/electron-shell/scripts/stage-cerul-api.mjs`. |

## Local Model Runtime

The base app should not bundle model weights. Local model support is optional
and should download runtime packs and model snapshots on demand. Release builds
may fetch reviewed, pinned model snapshots from the Cerul R2/CDN mirror before
falling back to the original upstream repository.

Release candidates must review the licenses and terms for every model snapshot
and Python wheel used by local runtime packs. In particular:

- MLX and MLX ecosystem packages must be compatible with redistribution if
  shipped in a runtime pack.
- Model weights must be checked for commercial-use, attribution, and usage-scale
  restrictions.
- macOS arm64 MLX local mode is the first supported local runtime target.

Current default local model mirror candidates:

- `mlx-community/Qwen3-VL-Embedding-2B-6bit` — Apache-2.0
- `Qwen/Qwen3-ASR-0.6B` — Apache-2.0
- `Qwen/Qwen3-ForcedAligner-0.6B` — Apache-2.0
- `PaddlePaddle/PP-OCRv6_small_det_onnx` — Apache-2.0
- `PaddlePaddle/PP-OCRv6_small_rec_onnx` — Apache-2.0
  Windows/Linux local runtimes require separate backend work and license review.

## Brand Assets

Cerul brand assets in this repository are included so the official app can be
built. They are not a general trademark license. See `TRADEMARKS.md`.
