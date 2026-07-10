#include "edge_bytecode_cache.h"

#define XXH_INLINE_ALL
#include "xxhash/xxhash.h"

#include <atomic>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <string>
#include <system_error>

#include "edge_bytecode_io.h"

namespace edge_bytecode_cache {
namespace {

constexpr char kMagic[8] = {'E', 'D', 'G', 'E', 'J', 'S', 'B', 'C'};
constexpr size_t kHeaderSize = 48;

// CLI cache override: 0 = none, +1 = force user sidecars on, -1 = force all off.
std::atomic<int> g_cli_enabled{0};

bool IsFalsyEnvValue(const char* value) {
  if (value == nullptr || value[0] == '\0') return false;
  std::string normalized(value);
  for (char& ch : normalized) {
    ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  }
  return normalized == "0" || normalized == "false" || normalized == "no" ||
         normalized == "off";
}

void Trace(const char* event, const std::string& path, const char* detail = nullptr) {
  if (!TraceEnabled()) return;
  if (detail != nullptr) {
    std::fprintf(stderr, "[edge-bytecode-cache] %s %s (%s)\n", event, path.c_str(), detail);
  } else {
    std::fprintf(stderr, "[edge-bytecode-cache] %s %s\n", event, path.c_str());
  }
}

using io::ReadU32;
using io::ReadU64;
using io::WriteU32;
using io::WriteU64;

}  // namespace

const char* SidecarSuffix() {
#if defined(EDGE_BUNDLED_NAPI_V8)
  return ".v8b";
#elif defined(EDGE_NAPI_QUICKJS)
  return ".qjsb";
#else
  return "";
#endif
}

const std::string& EngineCacheTag() {
  static const std::string tag = [] {
#if defined(EDGE_BUNDLED_NAPI_V8) && defined(EDGE_EMBEDDED_V8_VERSION)
    return std::string("v8-") + EDGE_EMBEDDED_V8_VERSION;
#elif defined(EDGE_NAPI_QUICKJS) && defined(EDGE_EMBEDDED_QUICKJS_VERSION)
    return std::string("qjs-ng-") + EDGE_EMBEDDED_QUICKJS_VERSION;
#else
    return std::string();
#endif
  }();
  return tag;
}

namespace {
// "13.6.233.17-node.0" -> "13-6"; "0.14.0" -> "0-14". Major.minor only, dots
// as dashes, so it stays filename-clean.
std::string MajorMinorDashed(const char* version) {
  std::string out;
  int dots = 0;
  for (const char* p = version; *p != '\0'; ++p) {
    if (*p == '.') {
      if (++dots >= 2) break;
      out.push_back('-');
    } else if ((*p >= '0' && *p <= '9')) {
      out.push_back(*p);
    } else {
      break;  // stop at the first non-numeric (e.g. "-node...")
    }
  }
  return out;
}
}  // namespace

const std::string& EngineFileTag() {
  // Short, human-readable engine tag for the cache *filename* only (the header
  // keeps the full EngineCacheTag for the precise staleness check). Coarser
  // than the header tag, so two patch builds sharing a major.minor reuse one
  // filename — the header still rejects the stale one and rewrites.
  static const std::string tag = [] {
#if defined(EDGE_BUNDLED_NAPI_V8) && defined(EDGE_EMBEDDED_V8_VERSION)
    return std::string("v8-") + MajorMinorDashed(EDGE_EMBEDDED_V8_VERSION);
#elif defined(EDGE_NAPI_QUICKJS) && defined(EDGE_EMBEDDED_QUICKJS_VERSION)
    return std::string("qjs-") + MajorMinorDashed(EDGE_EMBEDDED_QUICKJS_VERSION);
#else
    return std::string();
#endif
  }();
  return tag;
}

uint64_t Hash64(const void* data, size_t size) {
  return XXH3_64bits(data, size);
}

// Cache control is a tri-state: -1 force-off (kill switch), +1 force-on
// (opt-in user sidecars), 0 default. The CLI override (set from flags) wins
// over the EDGE_BYTECODE_CACHE env var, which wins over the built-in default.
//
//   builtins cache  : ON  unless something forces off (default-on, our own
//                     tested lib code; the biggest startup win at low risk).
//   user sidecars   : OFF unless something forces on (opt-in, since arbitrary
//                     user code is less battle-tested).
namespace {
int EnvOverride() {
  static const int v = [] {
    const char* e = std::getenv("EDGE_BYTECODE_CACHE");
    if (e == nullptr || e[0] == '\0') return 0;
    return IsFalsyEnvValue(e) ? -1 : 1;  // explicit falsy kills; truthy opts in
  }();
  return v;
}
int EffectiveOverride() {
  const int cli = g_cli_enabled.load(std::memory_order_relaxed);
  return cli != 0 ? cli : EnvOverride();
}
}  // namespace

void SetCacheDisabledFromCli() {
  g_cli_enabled.store(-1, std::memory_order_relaxed);
}

void SetSidecarsEnabledFromCli() {
  g_cli_enabled.store(1, std::memory_order_relaxed);
}

bool BuiltinsCacheEnabled() {
  return !EngineCacheTag().empty() && EffectiveOverride() != -1;
}

bool Enabled() {
  return !EngineCacheTag().empty() && EffectiveOverride() == 1;
}

bool TraceEnabled() {
  static const bool enabled = [] {
    const char* value = std::getenv("EDGE_BYTECODE_CACHE_TRACE");
    return value != nullptr && value[0] != '\0' && !IsFalsyEnvValue(value);
  }();
  return enabled;
}

std::string SidecarPathForSource(const std::string& source_path) {
  // Caches live in a per-directory "__edgecache__" subdir (PEP 3147
  // __pycache__ style): "dir/app.js" -> "dir/__edgecache__/app.js.<file-tag>.jsc"
  // (e.g. "app.js.v8-13-6.jsc" / "app.mjs.qjs-0-14.jsc"). The full source
  // filename (extension included) keys the entry, so it is unique within the
  // directory — ".js"/".mjs"/".cjs" siblings never collide. The short engine
  // tag lets different engines/major.minor versions coexist; the source
  // identity and full engine version are still validated by the header.
  const std::filesystem::path source(source_path);
  const std::string name = source.filename().string() + "." + EngineFileTag() + ".jsc";
  return (source.parent_path() / "__edgecache__" / name).string();
}

// Engine-agnostic header (48 bytes): the payload is opaque self-validating
// engine data (V8 CachedData; QuickJS bytecode behind the provider's QJSB
// header), so the container carries only source identity and structure.
std::vector<uint8_t> EncodeSidecar(std::string_view engine_tag,
                                   std::string_view source_utf8,
                                   uint32_t flags,
                                   const uint8_t* payload,
                                   size_t payload_size) {
  std::vector<uint8_t> out(kHeaderSize + engine_tag.size() + payload_size);
  std::memcpy(out.data(), kMagic, sizeof(kMagic));
  WriteU32(&out, 8, kFormatVersion);
  WriteU32(&out, 12, flags);
  WriteU64(&out, 16, source_utf8.size());
  WriteU64(&out, 24, Hash64(source_utf8.data(), source_utf8.size()));
  WriteU64(&out, 32, payload_size);
  WriteU32(&out, 40, static_cast<uint32_t>(engine_tag.size()));
  WriteU32(&out, 44, 0);  // reserved
  std::memcpy(out.data() + kHeaderSize, engine_tag.data(), engine_tag.size());
  if (payload_size > 0) {
    std::memcpy(out.data() + kHeaderSize + engine_tag.size(), payload, payload_size);
  }
  return out;
}

bool DecodeSidecar(const uint8_t* data,
                   size_t size,
                   std::string_view engine_tag,
                   std::string_view source_utf8,
                   uint32_t expected_flags,
                   size_t* payload_offset_out,
                   size_t* payload_size_out) {
  if (payload_offset_out == nullptr || payload_size_out == nullptr) return false;
  *payload_offset_out = 0;
  *payload_size_out = 0;
  if (data == nullptr || size < kHeaderSize) return false;
  if (std::memcmp(data, kMagic, sizeof(kMagic)) != 0) return false;
  if (ReadU32(data, 8) != kFormatVersion) return false;
  if (ReadU32(data, 12) != expected_flags) return false;

  const uint64_t source_len = ReadU64(data, 16);
  const uint64_t source_hash = ReadU64(data, 24);
  const uint64_t payload_len = ReadU64(data, 32);
  const uint64_t tag_len = ReadU32(data, 40);

  // Bounds first: the tag/payload lengths come from untrusted file bytes, so
  // validate the structure additively (never `kHeaderSize + tag_len +
  // payload_len`, which can wrap past 2^64 and let an out-of-bounds tag/payload
  // span through). `size >= kHeaderSize` is already established above.
  if (tag_len > size - kHeaderSize) return false;
  if (payload_len != size - kHeaderSize - tag_len) return false;
  if (tag_len != engine_tag.size() ||
      std::memcmp(data + kHeaderSize, engine_tag.data(), engine_tag.size()) != 0) {
    return false;
  }
  if (source_len != source_utf8.size() ||
      source_hash != Hash64(source_utf8.data(), source_utf8.size())) {
    return false;
  }

  *payload_offset_out = kHeaderSize + tag_len;
  *payload_size_out = payload_len;
  return true;
}

bool ReadSidecar(const std::string& source_path,
                 std::string_view source_utf8,
                 uint32_t expected_flags,
                 SidecarPayload* out) {
  if (out == nullptr) return false;
  out->file_bytes.clear();
  out->payload_offset = 0;
  out->payload_size = 0;
  if (!Enabled()) return false;

  const auto started = std::chrono::steady_clock::now();
  const std::string sidecar_path = SidecarPathForSource(source_path);

  // One sized read; validation happens in place over the same buffer. A
  // missing file is the common no-cache case, not a read error.
  if (!io::ReadFileFully(sidecar_path, &out->file_bytes)) {
    if (std::filesystem::exists(sidecar_path)) Trace("miss", sidecar_path, "read-error");
    return false;
  }

  if (!DecodeSidecar(out->file_bytes.data(), out->file_bytes.size(), EngineCacheTag(),
                     source_utf8, expected_flags,
                     &out->payload_offset, &out->payload_size)) {
    out->file_bytes.clear();
    Trace("miss", sidecar_path, "invalid-or-stale");
    return false;
  }
  if (TraceEnabled()) {
    const auto micros = std::chrono::duration_cast<std::chrono::microseconds>(
                            std::chrono::steady_clock::now() - started)
                            .count();
    char detail[64];
    std::snprintf(detail, sizeof(detail), "read+hash=%lldus payload=%zub",
                  static_cast<long long>(micros), out->payload_size);
    Trace("hit", sidecar_path, detail);
  }
  return true;
}

bool WriteSidecar(const std::string& source_path,
                  std::string_view source_utf8,
                  uint32_t flags,
                  const uint8_t* payload,
                  size_t payload_size) {
  if (!Enabled() || payload == nullptr || payload_size == 0) return false;

  const std::string sidecar_path = SidecarPathForSource(source_path);
  const std::vector<uint8_t> contents =
      EncodeSidecar(EngineCacheTag(), source_utf8, flags, payload, payload_size);

  // Ensure the __edgecache__ subdir exists (idempotent). The atomic write's
  // temp file lives inside it, so it must be present first. A read-only tree
  // makes this fail silently, like the write itself.
  std::error_code dir_ec;
  std::filesystem::create_directories(std::filesystem::path(sidecar_path).parent_path(), dir_ec);

  if (!io::AtomicWriteFile(sidecar_path, contents.data(), contents.size())) {
    Trace("write-failed", sidecar_path);
    return false;
  }
  Trace("write", sidecar_path);
  return true;
}

bool RemoveSidecar(const std::string& source_path) {
  const std::string sidecar_path = SidecarPathForSource(source_path);
  std::error_code ec;
  const bool removed = std::filesystem::remove(sidecar_path, ec);
  if (removed) Trace("remove", sidecar_path);
  return removed && !ec;
}

}  // namespace edge_bytecode_cache
