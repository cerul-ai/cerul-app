# Contributing to Cerul App

Cerul App is Apache-2.0 licensed and currently in alpha. Contributions are
welcome, but release-sensitive areas need extra care.

## Development

```bash
cp .env.example .env
pnpm install
./run.sh
```

Use `./run.sh` for normal cached development. Use `./rebuild.sh` when you need a
full clean rebuild.

## Checks

Before opening a PR, run the checks relevant to your change:

```bash
pnpm --filter @cerul/desktop build
pnpm --filter @cerul/electron-shell build
cargo check --workspace -j 1
scripts/smoke.sh
```

Release or installer changes should also run:

```bash
scripts/smoke-electron-shell.sh
scripts/smoke-release.sh --debug-installers
```

## Boundaries

This repository contains the local desktop app, local API, indexing pipeline,
search, scripts, and public docs.

Do not add:

- Cerul Cloud server code
- billing, quota, or entitlement internals
- signing certificates, Apple credentials, or CI secrets
- private design packages or unpublished marketing drafts
- model weights or large runtime caches

The desktop account UI may call public Cerul Cloud endpoints, but the cloud
service implementation stays private.

## Licensing

New code should be compatible with Apache-2.0. New bundled binaries, model
weights, Python wheels, or generated assets must be reviewed before release and
documented in `THIRD_PARTY_LICENSES.md`.

