#ifndef EDGE_BUILTIN_BYTECODE_H_
#define EDGE_BUILTIN_BYTECODE_H_

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

// Consolidated bytecode cache for embedded builtins, stored in a single file
// next to the binary ("edge" -> "edge.builtins.v8b" / "edge.builtins.qjsb").
// Builtins ship inside the binary, so unlike user-file sidecars there is one
// store per installed binary, loaded once per process and flushed (merged)
// once at teardown. Payloads are opaque engine data; this module owns the
// container format, validation, and the process-global entry store only.
namespace edge_builtin_bytecode {

// Bump when the container layout or the payload encoding changes (v2: raw
// payloads; v3: entries dropped the engine-conditional payload hash — engine
// payloads self-validate, so the container carries source identity only).
constexpr uint32_t kFormatVersion = 3;

// Compile shape of an entry. Each kind pins the compile path, parameter list,
// and wrapper that produced the payload; the entry's source hash covers the
// exact string handed to the engine compiler (raw builtin source for the
// function kinds, the full wrapped IIFE text for the wrapper kinds), so any
// wrapper or parameter drift self-invalidates.
enum class Kind : uint8_t {
  kFnPerContext = 0,      // [exports, primordials, privateSymbols, perIsolateSymbols]
  kFnBootstrapRealm = 1,  // [process, getLinkedBinding, getInternalBinding, primordials]
  kFnBootstrapMain = 2,   // [process, require, internalBinding, primordials]
  kFnLazyBuiltin = 3,     // [exports, require, module, process, internalBinding, primordials]
  kWrapperStandard = 4,   // (function(internalBinding, primordials){...}) script
  kWrapperPerContext = 5, // (function(primordials, privateSymbols, perIsolateSymbols){...}) script
};

// View into the loaded store's buffer; valid for the process lifetime.
struct PayloadView {
  const uint8_t* data = nullptr;
  size_t size = 0;
};

// Cache file path: process executable path + ".builtins" + engine suffix.
std::string BuiltinCacheFilePath();

// Looks up (kind, id) in the consolidated cache, validating the entry was
// compiled from exactly source_utf8. Loads the file lazily on first call
// (one read, validated in place). False on miss/disabled; never throws.
bool TryGet(Kind kind,
            std::string_view id,
            std::string_view source_utf8,
            PayloadView* out);

// Queues serialized bytecode for (kind, id) to be persisted by FlushIfDirty.
// Workers bootstrap concurrently with the main thread; safe from any thread.
void Record(Kind kind,
            std::string_view id,
            std::string_view source_utf8,
            const uint8_t* payload,
            size_t payload_size);

// Writes loaded-merged-with-recorded entries atomically (temp file + rename)
// if anything was recorded this run. Failures (read-only install dir, ...)
// are silent: the next run simply recompiles.
void FlushIfDirty();

// Serialization helpers exposed for tests.
struct FileEntry {
  Kind kind = Kind::kFnLazyBuiltin;
  std::string id;
  uint64_t source_hash = 0;
  std::vector<uint8_t> payload;
};
std::vector<uint8_t> EncodeFile(std::string_view engine_tag,
                                const std::vector<FileEntry>& entries);
bool DecodeFile(const uint8_t* data,
                size_t size,
                std::string_view engine_tag,
                std::vector<FileEntry>* out);

}  // namespace edge_builtin_bytecode

#endif  // EDGE_BUILTIN_BYTECODE_H_
