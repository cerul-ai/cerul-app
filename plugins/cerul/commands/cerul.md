---
description: Search the local Cerul video library and cite results with timestamps
---

Search the user's local Cerul video library for: $ARGUMENTS

Follow the `cerul-video-search` skill in this plugin: call
`POST http://127.0.0.1:23785/v1/search` with the query above, then present the
top results as citations — `item.title` + `time.timestamp` + `text.quote`, each
with its `evidence.open_in_cerul` link. If the API is unreachable, tell the
user to open the Cerul desktop app and retry. If $ARGUMENTS is empty, ask what
to search for.
