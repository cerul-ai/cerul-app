# Cerul Local Model Mirror

Cerul's macOS local runtime can download pinned model snapshots from the Cerul
R2/CDN mirror before falling back to Hugging Face.

## Storage

- R2 bucket: `cerul-cdn`
- Public domain: `https://cdn.cerul.ai`
- Manifest URL: `https://cdn.cerul.ai/models/v1/manifest.json`
- Archive layout: `models/v1/models--<namespace>--<repo>/<revision>/snapshot.tar.gz`
- Chunk layout: `models/v1/models--<namespace>--<repo>/<revision>/snapshot.tar.gz.part-000`

The mirror stores model archives in R2, not in a database. A database or KV store
is only appropriate for metadata; today the app uses the static JSON manifest.
Each model is mirrored separately in its own directory. Large model archives are
split into per-model chunks only because Wrangler remote uploads are limited to
objects below 300 MiB; chunks are not shared across models and no combined model
bundle is produced.

## Build and Upload

```bash
scripts/mirror-local-models.py --upload
```

The script reads pinned model revisions from
`mlx-sidecar/cerul_mlx_sidecar.py`, downloads snapshots with
`huggingface_hub`, creates plain `snapshot.tar.gz` archives, splits each archive
into per-model chunks, computes SHA256 for the full archive and every chunk,
uploads the chunks to R2, then uploads `manifest.json`.
The build script defaults to Hugging Face's classic HTTP path
(`HF_HUB_DISABLE_XET=1`) because Xet stalled in the current release network
environment. Set `HF_HUB_DISABLE_XET=0` explicitly if a future builder wants to
test Xet again.

By default, the script mirrors the four release-path models:

- `mlx-community/Qwen3-VL-Embedding-2B-6bit`
- `Qwen/Qwen3-ASR-0.6B`
- `Qwen/Qwen3-ForcedAligner-0.6B`
- `mlx-community/Qwen3-VL-2B-Instruct-4bit`

`mlx-community/whisper-large-v3-turbo` is intentionally excluded from the
default mirror batch because its MLX model card does not declare a license tag.
Use `--include-whisper` only after refreshing the license review.

## Runtime Controls

- `CERUL_MODEL_MIRROR_BASE_URL`: override the manifest base URL.
- `CERUL_DISABLE_MODEL_MIRROR=1`: skip the mirror and use Hugging Face.
- `CERUL_MODEL_MIRROR_TIMEOUT_SECS`: network timeout for manifest/archive reads.

Downloaded mirror snapshots live under:

```text
~/Library/Application Support/Cerul/models/mlx/cerul-mirror/
```

If the manifest is unavailable, a checksum fails, or an archive is incomplete,
the sidecar logs the mirror error and falls back to Hugging Face.
