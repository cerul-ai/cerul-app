---
description: Search the local Cerul video library and cite results with timestamps
---

Search the user's local Cerul video library for: $ARGUMENTS

Follow the `cerul-video-search` skill in this plugin: call
`POST http://127.0.0.1:23785/v1/search` with the query above, then present the
top results as citations — `item.title` + `time.timestamp` + `text.quote`, each
with its `evidence.open_in_cerul` link. 23785 is Cerul's default API port; if
the request is refused, first ask whether the user changed the port in Cerul's
settings (Advanced) and retry with that port, and if the port is unchanged,
tell the user to open the Cerul desktop app and retry. If $ARGUMENTS is empty,
ask what to search for.
