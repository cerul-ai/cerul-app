# Privacy Notes

Cerul App is designed as a local-first desktop app. Local search works without a
Cerul Cloud account.

## Local Data

The app stores local indexes, settings, media cache, and model/runtime cache on
the user's machine. Indexed media and search data are not uploaded to Cerul
Cloud by default.

## Remote Providers

When users configure Remote API providers or request cloud analysis, media or
derived content may be sent to the selected provider. Provider requests can
incur cost and are subject to that provider's terms.

## Cerul Cloud Account

The desktop app includes an optional account UI for Cerul Cloud features such as
managed credits, sync, and Pro capabilities. The Cerul Cloud service
implementation is not part of this open-source repository.

Account tokens are stored locally through the desktop shell store. Do not share
logs containing account tokens or provider API keys.

## Local Models

Optional local model runtimes may download model files to a local cache. Model
weights are not bundled in the base app by default.

