const DEFAULT_RUNTIME_TTY_CONFIG = {
  stdinIsTTY: false,
  stdoutIsTTY: false,
  stderrIsTTY: false,
  cols: 80,
  rows: 24
};

var _cachedRuntimeTtyConfig;

function _kernelIsatty(fd) {
  if (typeof _kernelIsattyRaw === "undefined") {
    throw new Error("_kernelIsattyRaw is unavailable");
  }
  return _kernelIsattyRaw.applySync(void 0, [fd]) === true;
}

function _kernelTtySize(fd) {
  if (typeof _kernelTtySizeRaw === "undefined") {
    throw new Error("_kernelTtySizeRaw is unavailable");
  }
  const size = _kernelTtySizeRaw.applySync(void 0, [fd]);
  if (!size || typeof size.cols !== "number" || typeof size.rows !== "number") {
    throw new Error("_kernelTtySizeRaw returned an invalid size");
  }
  return size;
}

function _resolveRuntimeTtyConfig() {
  if (_cachedRuntimeTtyConfig) {
    return _cachedRuntimeTtyConfig;
  }
  if (typeof __runtimeTtyConfig !== "undefined") {
    _cachedRuntimeTtyConfig = __runtimeTtyConfig;
    return _cachedRuntimeTtyConfig;
  }
  try {
    _cachedRuntimeTtyConfig = {
      stdinIsTTY: _kernelIsatty(0),
      stdoutIsTTY: _kernelIsatty(1),
      stderrIsTTY: _kernelIsatty(2),
      cols: 80,
      rows: 24
    };
  } catch {
    // Snapshot/bootstrap evaluation can touch process stdio before the kernel
    // sync bridge is attached. Return the safe default for that early read, but
    // do not cache it: the execution must retry once the bridge is available.
    return DEFAULT_RUNTIME_TTY_CONFIG;
  }
  for (const fd of [1, 0]) {
    try {
      const size = _kernelTtySize(fd);
      _cachedRuntimeTtyConfig.cols = size.cols;
      _cachedRuntimeTtyConfig.rows = size.rows;
      break;
    } catch {
    }
  }
  return _cachedRuntimeTtyConfig;
}

export { _resolveRuntimeTtyConfig };
