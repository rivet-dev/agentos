#ifndef EDGE_BYTECODE_CACHE_H_
#define EDGE_BYTECODE_CACHE_H_

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

// Sidecar bytecode caches: engine-serialized compiled code stored in a
// per-directory "__edgecache__" subdir next to the source, PEP 3147 style
// ("dir/app.js" -> "dir/__edgecache__/app.js.v8-13-6.jsc"). The payload is
// opaque engine data (V8 code cache / QuickJS JS_WriteObject bytecode); this
// module owns the container format and its validation only.
namespace edge_bytecode_cache {

// Bump whenever the bytes handed to the engine compiler for a given source
// file change shape (CJS wrapper text, parameter list, shebang handling, ...)
// or the container/payload encoding changes (v2: XXH3 instead of FNV-1a;
// v3: payloads lost the in-provider source-hash prefix; v4: engine-agnostic
// 48-byte header — engine-specific validation lives inside the QuickJS
// payload itself, mirroring V8's CachedData).
constexpr uint32_t kFormatVersion = 4;

// Compile shape of the payload; a sidecar is only consumed by the exact shape
// that produced it.
// bit 0: CJS function-compile with params (exports, require, module,
//        __filename, __dirname).
constexpr uint32_t kFlagCjsFunctionV1 = 1u << 0;
// bit 1: ES module compile (ModuleWrap / module shape).
constexpr uint32_t kFlagEsmModuleV1 = 1u << 1;

// Engine suffix for the consolidated builtins cache file next to the binary
// (".v8b" / ".qjsb"). User-file sidecars use the
// __edgecache__/<filename>.<tag>.jsc scheme instead; see SidecarPathForSource.
// Empty when the active NAPI provider has no bytecode-cache support.
const char* SidecarSuffix();

// Engine identity baked into sidecar headers, e.g. "v8-11.9.169.7-node.0" or
// "qjs-ng-0.14.0". Empty disables the cache entirely.
const std::string& EngineCacheTag();

// Short engine tag for the cache *filename* only (e.g. "v8-13-6" / "qjs-0-14").
// Coarser than EngineCacheTag (engine + major.minor); the header still carries
// the full version for the staleness check.
const std::string& EngineFileTag();

// XXH3-64 over arbitrary bytes (container hashing).
uint64_t Hash64(const void* data, size_t size);

// CLI overrides (win over the EDGE_BYTECODE_CACHE env var):
//   --no-bytecode-cache / --check  -> SetCacheDisabledFromCli (kill switch)
//   --bytecode-cache / --precompile -> SetSidecarsEnabledFromCli (opt in)
void SetCacheDisabledFromCli();
void SetSidecarsEnabledFromCli();

// True when the consolidated builtins (lib/) cache may be read/written. ON by
// default; off only via the kill switch (--no-bytecode-cache,
// EDGE_BYTECODE_CACHE=0, --check) or when the provider has no cache support.
bool BuiltinsCacheEnabled();

// True when per-file user sidecars may be read/written. OFF by default;
// on only when explicitly opted in (--bytecode-cache, EDGE_BYTECODE_CACHE
// truthy, or --precompile) and not killed.
bool Enabled();

// True when EDGE_BYTECODE_CACHE_TRACE is set (shared by the builtins cache).
bool TraceEnabled();

std::string SidecarPathForSource(const std::string& source_path);

// A validated sidecar: the whole file is read once and the payload is a view
// into that buffer (no intermediate copies).
struct SidecarPayload {
  std::vector<uint8_t> file_bytes;
  size_t payload_offset = 0;
  size_t payload_size = 0;
  const uint8_t* data() const { return file_bytes.data() + payload_offset; }
};

// Reads <source_path><suffix> and validates it against the exact source text
// about to be compiled and the expected compile shape. False (and empty
// payload) on any mismatch; never throws.
bool ReadSidecar(const std::string& source_path,
                 std::string_view source_utf8,
                 uint32_t expected_flags,
                 SidecarPayload* out);

// Atomically writes the sidecar next to the source (temp file + rename).
// Failures (read-only filesystem, permissions, ...) are silent: returns
// false, never throws.
bool WriteSidecar(const std::string& source_path,
                  std::string_view source_utf8,
                  uint32_t flags,
                  const uint8_t* payload,
                  size_t payload_size);

bool RemoveSidecar(const std::string& source_path);

// Serialization helpers exposed for tests.
std::vector<uint8_t> EncodeSidecar(std::string_view engine_tag,
                                   std::string_view source_utf8,
                                   uint32_t flags,
                                   const uint8_t* payload,
                                   size_t payload_size);
bool DecodeSidecar(const uint8_t* data,
                   size_t size,
                   std::string_view engine_tag,
                   std::string_view source_utf8,
                   uint32_t expected_flags,
                   size_t* payload_offset_out,
                   size_t* payload_size_out);

}  // namespace edge_bytecode_cache

#endif  // EDGE_BYTECODE_CACHE_H_
