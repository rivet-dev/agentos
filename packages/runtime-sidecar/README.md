# @rivet-dev/agentos-runtime-sidecar

Platform-specific resolver for the agentOS native sidecar binary.

The compiled `agentos-native-sidecar` binary ships inside one of the
`@rivet-dev/agentos-runtime-sidecar-<platform>` packages. npm installs only the package
matching the current `os`/`cpu`/`libc` at install time.
The resolver requires that platform package to have the same version as this
package and to contain the binary. Reinstall agentOS if either check fails;
mixing sidecar versions is unsupported.

```js
const { getSidecarPath } = require("@rivet-dev/agentos-runtime-sidecar");

const binaryPath = getSidecarPath();
```

Set `AGENTOS_SIDECAR_BIN` to an absolute path to override resolution for
development or custom builds.

Supported platforms: `linux-x64-gnu`, `linux-arm64-gnu`, `darwin-x64`,
`darwin-arm64`.
