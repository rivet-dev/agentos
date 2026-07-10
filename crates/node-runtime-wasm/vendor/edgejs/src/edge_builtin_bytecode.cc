#include "edge_builtin_bytecode.h"

#include <cstdio>
#include <cstring>
#include <map>
#include <mutex>
#include <string>
#include <utility>

#include "edge_bytecode_cache.h"
#include "edge_bytecode_io.h"
#include "edge_process.h"

namespace edge_builtin_bytecode {
namespace {

using edge_bytecode_cache::io::ReadU16;
using edge_bytecode_cache::io::ReadU32;
using edge_bytecode_cache::io::ReadU64;
using edge_bytecode_cache::io::WriteU16;
using edge_bytecode_cache::io::WriteU32;
using edge_bytecode_cache::io::WriteU64;

constexpr char kMagic[8] = {'E', 'D', 'G', 'E', 'J', 'S', 'B', 'B'};
constexpr size_t kHeaderSize = 32;
// kind u8, reserved u8, id_len u16, payload_len u32, source_xxh3 u64.
constexpr size_t kEntryFixedSize = 16;
constexpr size_t kMaxKind = 5;

void Trace(const char* event, const std::string& subject, const char* detail = nullptr) {
  if (!edge_bytecode_cache::TraceEnabled()) return;
  if (detail != nullptr) {
    std::fprintf(stderr, "[edge-bytecode-cache] %s %s (%s)\n", event, subject.c_str(), detail);
  } else {
    std::fprintf(stderr, "[edge-bytecode-cache] %s %s\n", event, subject.c_str());
  }
}

// Walks every entry in a decoded-from-disk file. fn(kind, id, source_hash,
// payload_offset, payload_size); a malformed entry aborts the walk. Returns
// false on any structural problem. Payload integrity is the engine's job
// (V8 CachedData / the QuickJS provider's QJSB payload header).
template <typename Fn>
bool ForEachEntry(const uint8_t* data,
                  size_t size,
                  std::string_view engine_tag,
                  Fn fn) {
  if (data == nullptr || size < kHeaderSize) return false;
  if (std::memcmp(data, kMagic, sizeof(kMagic)) != 0) return false;
  if (ReadU32(data, 8) != kFormatVersion) return false;
  const uint32_t entry_count = ReadU32(data, 12);
  const uint32_t tag_len = ReadU32(data, 16);
  if (size < kHeaderSize + tag_len) return false;
  if (tag_len != engine_tag.size() ||
      std::memcmp(data + kHeaderSize, engine_tag.data(), engine_tag.size()) != 0) {
    return false;
  }

  size_t offset = kHeaderSize + tag_len;
  for (uint32_t i = 0; i < entry_count; ++i) {
    if (size - offset < kEntryFixedSize) return false;
    const uint8_t kind = data[offset];
    const uint16_t id_len = ReadU16(data, offset + 2);
    const uint32_t payload_len = ReadU32(data, offset + 4);
    const uint64_t source_hash = ReadU64(data, offset + 8);
    offset += kEntryFixedSize;
    if (kind > kMaxKind) return false;
    if (size - offset < static_cast<size_t>(id_len) + payload_len) return false;
    const std::string_view id(reinterpret_cast<const char*>(data + offset), id_len);
    offset += id_len;
    fn(static_cast<Kind>(kind), id, source_hash, offset, static_cast<size_t>(payload_len));
    offset += payload_len;
  }
  return offset == size;
}

std::string EntryKey(Kind kind, std::string_view id) {
  std::string key;
  key.reserve(id.size() + 1);
  key.push_back(static_cast<char>(kind));
  key.append(id);
  return key;
}

struct LoadedEntry {
  uint64_t source_hash = 0;
  size_t payload_offset = 0;
  size_t payload_size = 0;
};

struct DirtyEntry {
  uint64_t source_hash = 0;
  std::vector<uint8_t> payload;
};

// Process-global: workers bootstrap concurrently against the same store and
// the main env teardown flushes once for everyone. Intentionally leaked (the
// codebase avoids static destructors).
struct Store {
  std::mutex mutex;
  bool load_attempted = false;
  std::vector<uint8_t> file_bytes;            // backs every PayloadView handed out
  std::map<std::string, LoadedEntry> loaded;  // key: kind byte + id
  std::map<std::string, DirtyEntry> dirty;
};

Store& GetStore() {
  static Store* store = new Store();
  return *store;
}

// Caller holds store.mutex.
void EnsureLoadedLocked(Store& store) {
  if (store.load_attempted) return;
  store.load_attempted = true;

  const std::string path = BuiltinCacheFilePath();
  // Missing file is the common no-cache case (first run); not a load failure.
  if (!edge_bytecode_cache::io::ReadFileFully(path, &store.file_bytes)) {
    return;
  }

  const uint8_t* data = store.file_bytes.data();
  const bool ok = ForEachEntry(
      data, store.file_bytes.size(), edge_bytecode_cache::EngineCacheTag(),
      [&](Kind kind, std::string_view id, uint64_t source_hash, size_t payload_offset,
          size_t payload_size) {
        store.loaded[EntryKey(kind, id)] = LoadedEntry{source_hash, payload_offset, payload_size};
      });
  if (!ok) {
    store.loaded.clear();
    store.file_bytes.clear();
    Trace("builtins-load-failed", path, "invalid-or-stale");
    return;
  }
  if (edge_bytecode_cache::TraceEnabled()) {
    char detail[48];
    std::snprintf(detail, sizeof(detail), "entries=%zu bytes=%zu", store.loaded.size(),
                  store.file_bytes.size());
    Trace("builtins-load", path, detail);
  }
}

}  // namespace

std::string BuiltinCacheFilePath() {
  return EdgeGetProcessExecPath() + ".builtins" + edge_bytecode_cache::SidecarSuffix();
}

bool TryGet(Kind kind, std::string_view id, std::string_view source_utf8, PayloadView* out) {
  if (out == nullptr) return false;
  out->data = nullptr;
  out->size = 0;
  if (!edge_bytecode_cache::BuiltinsCacheEnabled()) return false;

  Store& store = GetStore();
  std::lock_guard<std::mutex> lock(store.mutex);
  EnsureLoadedLocked(store);
  const auto it = store.loaded.find(EntryKey(kind, id));
  if (it == store.loaded.end()) {
    Trace("builtin-miss", std::string(id));
    return false;
  }
  if (it->second.source_hash !=
      edge_bytecode_cache::Hash64(source_utf8.data(), source_utf8.size())) {
    Trace("builtin-miss", std::string(id), "source-mismatch");
    return false;
  }
  out->data = store.file_bytes.data() + it->second.payload_offset;
  out->size = it->second.payload_size;
  Trace("builtin-hit", std::string(id));
  return true;
}

void Record(Kind kind,
            std::string_view id,
            std::string_view source_utf8,
            const uint8_t* payload,
            size_t payload_size) {
  if (!edge_bytecode_cache::BuiltinsCacheEnabled()) return;
  if (payload == nullptr || payload_size == 0) return;

  Store& store = GetStore();
  std::lock_guard<std::mutex> lock(store.mutex);
  DirtyEntry& entry = store.dirty[EntryKey(kind, id)];
  entry.source_hash = edge_bytecode_cache::Hash64(source_utf8.data(), source_utf8.size());
  entry.payload.assign(payload, payload + payload_size);
}

void FlushIfDirty() {
  if (!edge_bytecode_cache::BuiltinsCacheEnabled()) return;

  Store& store = GetStore();
  std::lock_guard<std::mutex> lock(store.mutex);
  if (store.dirty.empty()) return;
  EnsureLoadedLocked(store);

  // Merge: recorded entries supersede loaded ones with the same key.
  std::vector<FileEntry> entries;
  entries.reserve(store.loaded.size() + store.dirty.size());
  for (const auto& [key, loaded] : store.loaded) {
    if (store.dirty.count(key) != 0) continue;
    FileEntry entry;
    entry.kind = static_cast<Kind>(key[0]);
    entry.id = key.substr(1);
    entry.source_hash = loaded.source_hash;
    entry.payload.assign(store.file_bytes.data() + loaded.payload_offset,
                         store.file_bytes.data() + loaded.payload_offset + loaded.payload_size);
    entries.push_back(std::move(entry));
  }
  for (const auto& [key, dirty] : store.dirty) {
    FileEntry entry;
    entry.kind = static_cast<Kind>(key[0]);
    entry.id = key.substr(1);
    entry.source_hash = dirty.source_hash;
    entry.payload = dirty.payload;
    entries.push_back(std::move(entry));
  }

  const std::string path = BuiltinCacheFilePath();
  const std::vector<uint8_t> contents =
      EncodeFile(edge_bytecode_cache::EngineCacheTag(), entries);

  if (!edge_bytecode_cache::io::AtomicWriteFile(path, contents.data(), contents.size())) {
    Trace("builtins-write-failed", path);
    return;
  }
  store.dirty.clear();
  if (edge_bytecode_cache::TraceEnabled()) {
    char detail[48];
    std::snprintf(detail, sizeof(detail), "entries=%zu bytes=%zu", entries.size(),
                  contents.size());
    Trace("builtins-write", path, detail);
  }
}

std::vector<uint8_t> EncodeFile(std::string_view engine_tag,
                                const std::vector<FileEntry>& entries) {
  size_t total = kHeaderSize + engine_tag.size();
  for (const FileEntry& entry : entries) {
    total += kEntryFixedSize + entry.id.size() + entry.payload.size();
  }

  std::vector<uint8_t> out(total);
  std::memcpy(out.data(), kMagic, sizeof(kMagic));
  WriteU32(&out, 8, kFormatVersion);
  WriteU32(&out, 12, static_cast<uint32_t>(entries.size()));
  WriteU32(&out, 16, static_cast<uint32_t>(engine_tag.size()));
  // 20..31 reserved (zeroed).
  std::memcpy(out.data() + kHeaderSize, engine_tag.data(), engine_tag.size());

  size_t offset = kHeaderSize + engine_tag.size();
  for (const FileEntry& entry : entries) {
    out[offset] = static_cast<uint8_t>(entry.kind);
    out[offset + 1] = 0;
    WriteU16(&out, offset + 2, static_cast<uint16_t>(entry.id.size()));
    WriteU32(&out, offset + 4, static_cast<uint32_t>(entry.payload.size()));
    WriteU64(&out, offset + 8, entry.source_hash);
    offset += kEntryFixedSize;
    std::memcpy(out.data() + offset, entry.id.data(), entry.id.size());
    offset += entry.id.size();
    std::memcpy(out.data() + offset, entry.payload.data(), entry.payload.size());
    offset += entry.payload.size();
  }
  return out;
}

bool DecodeFile(const uint8_t* data,
                size_t size,
                std::string_view engine_tag,
                std::vector<FileEntry>* out) {
  if (out == nullptr) return false;
  out->clear();
  return ForEachEntry(
      data, size, engine_tag,
      [&](Kind kind, std::string_view id, uint64_t source_hash, size_t payload_offset,
          size_t payload_size) {
        FileEntry entry;
        entry.kind = kind;
        entry.id.assign(id);
        entry.source_hash = source_hash;
        entry.payload.assign(data + payload_offset, data + payload_offset + payload_size);
        out->push_back(std::move(entry));
      });
}

}  // namespace edge_builtin_bytecode
