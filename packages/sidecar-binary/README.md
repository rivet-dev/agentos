# @rivet-dev/agent-os-sidecar

Platform-specific resolver for the Agent OS native sidecar binary.

The compiled `agent-os-sidecar` binary ships inside one of the
`@rivet-dev/agent-os-sidecar-<platform>` packages, which this package declares as
optional dependencies. npm installs only the package matching the current
`os`/`cpu`/`libc` at install time.

```js
const { getSidecarPath } = require("@rivet-dev/agent-os-sidecar");

const binaryPath = getSidecarPath();
```

Set `AGENT_OS_SIDECAR_BINARY` to an absolute path to override resolution (useful
for development or custom builds).

Supported platforms: `linux-x64-gnu`, `linux-arm64-gnu`.
