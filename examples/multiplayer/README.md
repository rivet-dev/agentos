---
title: "Multiplayer"
description: "Multiple clients observing one live session with shared output and collaborative input."
category: "Sessions & Permissions"
order: 6
---

Run one agent session and let several clients watch and drive it at once. Reach for this when a session needs more than one viewer — pair programming, a shared review session, or a dashboard mirroring what an agent is doing live.

## How it works

Clients connect to the same VM actor by id (`getOrCreate("shared-agent")`), so they share its durable sessions. Each connection subscribes to the same live event stream—`sessionEvent`, `processOutput`, and `shellData` all fan out to every connected client. One client can open the session and call `prompt`, while others observe the streaming response without driving it. Because the server fans events out from a single session, the `onSessionEvent` server hook still fires once per event regardless of how many clients are attached. Reconnecting clients receive new stream entries after subscribing and can recover missed durable entries with `readHistory({ sessionId })`.

## Run it

```sh
npm install
# terminal 1 — start the server
npx tsx server.ts
# terminal 2+ — attach observers / drivers
npx tsx collaborative.ts
```

Multiple clients print the same session events; an observer sees the driver's prompt response stream in real time.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/multiplayer
