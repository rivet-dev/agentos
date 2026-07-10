#include "builtin_catalog.h"

#include "edge_builtin_catalog_data.h"
#include "edge_process.h"

#include <algorithm>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <string_view>
#include <system_error>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace builtin_catalog {

namespace fs = std::filesystem;

namespace {

constexpr std::string_view kNodePrefix = "node:";
constexpr std::string_view kInternalDepsPrefix = "internal/deps/";
constexpr const char kNodeLibRootEnvVar[] = "EDGEJS_LIB_DIR";

bool PathExistsDirectory(const fs::path& path) {
  std::error_code ec;
  return fs::exists(path, ec) && fs::is_directory(path, ec);
}

void AppendPathCandidate(std::vector<fs::path>* out, const fs::path& candidate) {
  if (out == nullptr || candidate.empty()) return;
  out->push_back(candidate);
}

void AppendEnvPathCandidate(std::vector<fs::path>* out, const char* env_var) {
  if (out == nullptr || env_var == nullptr || *env_var == '\0') return;
  const char* value = std::getenv(env_var);
  if (value == nullptr || *value == '\0') return;
  AppendPathCandidate(out, fs::path(value));
}

std::vector<fs::path> NodeLibRootCandidates() {
  const fs::path source_root =
      fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  std::vector<fs::path> candidates;

  AppendEnvPathCandidate(&candidates, kNodeLibRootEnvVar);
  AppendPathCandidate(&candidates, fs::path("/node-lib"));

  const fs::path exec_path = fs::path(EdgeGetProcessExecPath()).lexically_normal();
  if (!exec_path.empty()) {
    const fs::path install_root = exec_path.parent_path().parent_path();
    AppendPathCandidate(&candidates, install_root / "lib");
  }

  AppendPathCandidate(&candidates, source_root / "lib");

  std::error_code ec;
  const fs::path cwd = fs::current_path(ec);
  if (!ec && !cwd.empty()) {
    AppendPathCandidate(&candidates, cwd / "lib");
    AppendPathCandidate(&candidates, cwd.parent_path() / "lib");
  }

  return candidates;
}

std::vector<fs::path> NodeDepsRootCandidates() {
  const fs::path source_root =
      fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  std::vector<fs::path> candidates;

  const char* node_lib_root_env = std::getenv(kNodeLibRootEnvVar);
  if (node_lib_root_env != nullptr && *node_lib_root_env != '\0') {
    AppendPathCandidate(&candidates, fs::path(node_lib_root_env) / "internal" / "deps");
  }

  const fs::path exec_path = fs::path(EdgeGetProcessExecPath()).lexically_normal();
  if (!exec_path.empty()) {
    const fs::path install_root = exec_path.parent_path().parent_path();
    AppendPathCandidate(&candidates, install_root / "lib" / "internal" / "deps");
    AppendPathCandidate(&candidates, install_root / "node" / "deps");
  }

  AppendPathCandidate(&candidates, source_root / "deps");

  std::error_code ec;
  const fs::path cwd = fs::current_path(ec);
  if (!ec && !cwd.empty()) {
    AppendPathCandidate(&candidates, cwd / "deps");
    AppendPathCandidate(&candidates, cwd.parent_path() / "deps");
  }
  return candidates;
}

std::string NormalizePathKey(const fs::path& path) {
  std::error_code ec;
  return fs::absolute(path, ec).lexically_normal().string();
}

struct BuiltinIndex {
  std::vector<std::string> all_ids;
  std::unordered_map<std::string, const generated::BuiltinEntry*> by_id;
  std::unordered_map<std::string, const generated::BuiltinEntry*> by_path;
  BuiltinCategories categories;
};

BuiltinCategories BuildBuiltinCategories(const std::vector<std::string>& ids) {
  std::unordered_set<std::string> can_be_required = {
      "internal/deps/cjs-module-lexer/lexer",
  };
  std::unordered_set<std::string> cannot_be_required = {
      "quic",
      "sqlite",
      "sys",
      "wasi",
      "internal/quic/quic",
      "internal/quic/symbols",
      "internal/quic/stats",
      "internal/quic/state",
      "internal/test/binding",
      "internal/v8_prof_polyfill",
      "internal/v8_prof_processor",
      "internal/webstorage",
  };

  std::vector<std::string_view> internal_prefixes = {
      "internal/bootstrap/",
      "internal/per_context/",
      "internal/deps/",
      "internal/main/",
  };

  cannot_be_required.insert("internal/util/inspector");
  cannot_be_required.insert("internal/inspector/network");
  cannot_be_required.insert("internal/inspector/network_http");
  cannot_be_required.insert("internal/inspector/network_http2");
  cannot_be_required.insert("internal/inspector/network_resources");
  cannot_be_required.insert("internal/inspector/network_undici");
  cannot_be_required.insert("internal/inspector_async_hook");
  cannot_be_required.insert("internal/inspector_network_tracking");

#if !defined(EDGE_BUNDLED_NAPI_V8) || !defined(EDGE_HAS_ICU)
  cannot_be_required.insert("trace_events");
#endif

  for (const std::string& id : ids) {
    for (std::string_view prefix : internal_prefixes) {
      if (id.starts_with(prefix) && can_be_required.count(id) == 0) {
        cannot_be_required.insert(id);
      }
    }
  }

  BuiltinCategories categories;
  categories.can_be_required.reserve(ids.size());
  categories.cannot_be_required.reserve(ids.size());
  for (const std::string& id : ids) {
    if (cannot_be_required.count(id) != 0) {
      categories.cannot_be_required.push_back(id);
    } else {
      categories.can_be_required.push_back(id);
    }
  }

  std::sort(categories.can_be_required.begin(), categories.can_be_required.end());
  std::sort(categories.cannot_be_required.begin(), categories.cannot_be_required.end());
  return categories;
}

const BuiltinIndex& GetBuiltinIndex() {
  static const BuiltinIndex index = []() {
    BuiltinIndex out;
    out.all_ids.reserve(generated::kBuiltinEntries.size());
    out.by_id.reserve(generated::kBuiltinEntries.size());
    out.by_path.reserve(generated::kBuiltinEntries.size());

    for (const generated::BuiltinEntry& entry : generated::kBuiltinEntries) {
      out.all_ids.emplace_back(entry.id);
      out.by_id.emplace(out.all_ids.back(), &entry);

      const fs::path root = entry.is_internal_dep ? NodeDepsRoot() : NodeLibRoot();
      const fs::path full_path = root / fs::path(std::string(entry.relative_path));
      out.by_path.emplace(NormalizePathKey(full_path), &entry);
    }

    out.categories = BuildBuiltinCategories(out.all_ids);
    return out;
  }();
  return index;
}

bool NormalizeBuiltinSpecifier(const std::string& specifier, std::string* out) {
  if (out == nullptr) return false;

  std::string id = specifier;
  if (id.starts_with(kNodePrefix)) {
    id.erase(0, kNodePrefix.size());
  }
  if (id.empty() || id.front() == '.') return false;

  const auto& index = GetBuiltinIndex();
  if (index.by_id.find(id) == index.by_id.end()) return false;
  *out = std::move(id);
  return true;
}

const generated::BuiltinEntry* LookupBuiltinByPath(const fs::path& resolved_path) {
  const auto& index = GetBuiltinIndex();
  const auto it = index.by_path.find(NormalizePathKey(resolved_path));
  if (it == index.by_path.end()) return nullptr;
  return it->second;
}

}  // namespace

const fs::path& NodeLibRoot() {
  static const fs::path root = []() {
    const std::vector<fs::path> candidates = NodeLibRootCandidates();
    for (const fs::path& candidate : candidates) {
      if (PathExistsDirectory(candidate)) return fs::absolute(candidate).lexically_normal();
    }
    if (!candidates.empty()) return fs::absolute(candidates.front()).lexically_normal();
    return fs::path();
  }();
  return root;
}

const fs::path& NodeDepsRoot() {
  static const fs::path root = []() {
    const std::vector<fs::path> candidates = NodeDepsRootCandidates();
    for (const fs::path& candidate : candidates) {
      if (PathExistsDirectory(candidate)) return fs::absolute(candidate).lexically_normal();
    }
    if (!candidates.empty()) return fs::absolute(candidates.front()).lexically_normal();
    return fs::path();
  }();
  return root;
}

bool ResolveBuiltinId(const std::string& specifier, fs::path* out_path) {
  if (out_path == nullptr) return false;

  std::string id;
  if (!NormalizeBuiltinSpecifier(specifier, &id)) return false;

  const auto& index = GetBuiltinIndex();
  const auto it = index.by_id.find(id);
  if (it == index.by_id.end()) return false;

  const generated::BuiltinEntry* entry = it->second;
  const fs::path root = entry->is_internal_dep ? NodeDepsRoot() : NodeLibRoot();
  *out_path = fs::absolute(root / fs::path(std::string(entry->relative_path))).lexically_normal();
  return true;
}

bool TryGetBuiltinIdForPath(const fs::path& resolved_path, std::string* out_id) {
  if (out_id == nullptr) return false;

  const generated::BuiltinEntry* entry = LookupBuiltinByPath(resolved_path);
  if (entry == nullptr) return false;

  *out_id = std::string(entry->id);
  return true;
}

bool TryReadBuiltinSource(const fs::path& resolved_path, std::string* out_source) {
  if (out_source == nullptr) return false;

  const generated::BuiltinEntry* entry = LookupBuiltinByPath(resolved_path);
  if (entry == nullptr) return false;

  *out_source = std::string(entry->source);
  return true;
}

bool TryReadBuiltinSource(const std::string& specifier, std::string* out_source) {
  if (out_source == nullptr) return false;

  std::string id;
  if (!NormalizeBuiltinSpecifier(specifier, &id)) return false;

  const auto& index = GetBuiltinIndex();
  const auto it = index.by_id.find(id);
  if (it == index.by_id.end()) return false;

  *out_source = std::string(it->second->source);
  return true;
}

const std::vector<std::string>& AllBuiltinIds() {
  return GetBuiltinIndex().all_ids;
}

const BuiltinCategories& GetBuiltinCategories() {
  return GetBuiltinIndex().categories;
}

}  // namespace builtin_catalog
