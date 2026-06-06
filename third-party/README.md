# Third-party binaries

`scripts/fetch-binaries.sh` stages platform binaries here:

```text
third-party/<target-triple>/ffmpeg
third-party/<target-triple>/yt-dlp
third-party/<target-triple>/qdrant
```

The staged binaries are generated artifacts and are ignored by git. Electron
copies this directory into the app bundle resources during installer builds and
injects the packaged paths into the Rust runtime with `CERUL_FFMPEG_PATH`,
`CERUL_YTDLP_PATH`, and `CERUL_QDRANT_BIN`. Cerul uses the bundled `qdrant`
binary as the local vector-index sidecar when the default local Qdrant URL is
not already running.
