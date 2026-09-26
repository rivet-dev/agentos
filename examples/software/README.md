---
title: "Software"
description: "Install immutable command packages in an agentOS VM."
category: "Runtime"
order: 5
---

The hosted actor installs an HTTPS `.aospkg` URL with
`software.install`, lists live installed packages with `software.list`, and
removes one by content-derived package ID with `software.uninstall`.

Set `AGENTOS_PACKAGE_URL` and optionally `AGENTOS_PACKAGE_DIGEST`, then run:

```bash
npm install
npx tsx client.ts
```

The `quickstart-node` and `quickstart-wasm` examples show the separate
embedded Core API, which also accepts trusted local artifact paths.
