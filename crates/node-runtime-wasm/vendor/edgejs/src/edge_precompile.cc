#include "edge_precompile.h"

#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>
#include <string_view>
#include <system_error>

#include "edge_bytecode_cache.h"
#include "edge_url.h"
#include "simdjson/simdjson.h"
#include "unofficial_napi.h"

namespace edge_precompile {
namespace {

namespace fs = std::filesystem;

// Nearest-package.json "type" lookup, mirroring the module loader's scope walk
// (the scope ends at a node_modules boundary). Results are cached per
// directory because directory trees compile many siblings.
class PackageTypeResolver {
 public:
  bool IsModuleScope(const fs::path& file_path) {
    return DirIsModuleScope(file_path.parent_path());
  }

 private:
  bool DirIsModuleScope(const fs::path& dir) {
    const std::string key = dir.string();
    auto it = cache_.find(key);
    if (it != cache_.end()) return it->second;

    bool is_module = false;
    if (dir.filename() != "node_modules" && !dir.empty()) {
      const fs::path package_json = dir / "package.json";
      std::string type;
      if (ReadPackageType(package_json, &type)) {
        is_module = type == "module";
      } else {
        const fs::path parent = dir.parent_path();
        if (!parent.empty() && parent != dir) {
          is_module = DirIsModuleScope(parent);
        }
      }
    }
    cache_.emplace(key, is_module);
    return is_module;
  }

  static bool ReadPackageType(const fs::path& package_json, std::string* type_out) {
    std::error_code ec;
    if (!fs::is_regular_file(package_json, ec) || ec) return false;
    std::ifstream in(package_json, std::ios::binary);
    if (!in.is_open()) return false;
    std::ostringstream ss;
    ss << in.rdbuf();
    const std::string source = ss.str();

    simdjson::ondemand::parser parser;
    simdjson::padded_string padded(source);
    simdjson::ondemand::document document;
    simdjson::ondemand::object main_object;
    if (parser.iterate(padded).get(document) != simdjson::SUCCESS ||
        document.get_object().get(main_object) != simdjson::SUCCESS) {
      // Invalid package.json still establishes the scope; treat as commonjs.
      type_out->clear();
      return true;
    }
    std::string_view type_value;
    if (main_object["type"].get_string().get(type_value) == simdjson::SUCCESS) {
      *type_out = std::string(type_value);
    } else {
      type_out->clear();
    }
    return true;
  }

  std::map<std::string, bool> cache_;
};

bool ReadFileUtf8(const fs::path& path, std::string* out) {
  std::ifstream in(path, std::ios::binary);
  if (!in.is_open()) return false;
  std::ostringstream ss;
  ss << in.rdbuf();
  *out = ss.str();
  return !in.bad();
}

std::string DescribePendingException(napi_env env) {
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) {
    return "unknown compile error";
  }
  napi_value exception = nullptr;
  if (napi_get_and_clear_last_exception(env, &exception) != napi_ok || exception == nullptr) {
    return "unknown compile error";
  }
  napi_value as_string = nullptr;
  if (napi_coerce_to_string(env, exception, &as_string) != napi_ok || as_string == nullptr) {
    return "unknown compile error";
  }
  size_t length = 0;
  if (napi_get_value_string_utf8(env, as_string, nullptr, 0, &length) != napi_ok) {
    return "unknown compile error";
  }
  std::string message(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, as_string, message.data(), message.size(), &copied) != napi_ok) {
    return "unknown compile error";
  }
  message.resize(copied);
  return message;
}

enum class FileResult {
  kWritten,
  kSkipped,
  kFailed,
};

enum class FileShape {
  kCjs,
  kEsm,
};

struct PrecompileEntry {
  fs::path path;
  FileShape shape = FileShape::kCjs;

  bool operator<(const PrecompileEntry& other) const { return path < other.path; }
  bool operator==(const PrecompileEntry& other) const { return path == other.path; }
};

FileResult PrecompileFile(napi_env env,
                          const fs::path& file_path,
                          FileShape shape,
                          std::string* detail_out) {
  std::string source;
  if (!ReadFileUtf8(file_path, &source)) {
    *detail_out = "failed to read file";
    return FileResult::kFailed;
  }

  napi_handle_scope scope = nullptr;
  if (napi_open_handle_scope(env, &scope) != napi_ok) {
    *detail_out = "failed to open handle scope";
    return FileResult::kFailed;
  }
  struct ScopeCloser {
    napi_env env;
    napi_handle_scope scope;
    ~ScopeCloser() { napi_close_handle_scope(env, scope); }
  } scope_closer{env, scope};

  const std::string path_utf8 = file_path.string();
  // ESM modules compile under their file:// URL — the exact name the runtime
  // ModuleWrap hook compiles with (and that QuickJS bakes into the bytecode).
  const std::string filename_utf8 =
      shape == FileShape::kEsm ? edge_url::PathToFileURLString(path_utf8) : path_utf8;
  if (filename_utf8.empty()) {
    *detail_out = "failed to derive module URL";
    return FileResult::kFailed;
  }

  napi_value code = nullptr;
  napi_value filename = nullptr;
  if (napi_create_string_utf8(env, source.c_str(), source.size(), &code) != napi_ok ||
      napi_create_string_utf8(env, filename_utf8.c_str(), filename_utf8.size(), &filename) != napi_ok) {
    *detail_out = "failed to create compile arguments";
    return FileResult::kFailed;
  }

  napi_value params = nullptr;
  if (shape == FileShape::kCjs) {
    // Same parameter list the CJS loader compiles with; the sidecar must match
    // the runtime compile shape exactly.
    static constexpr const char* kParams[] = {"exports", "require", "module", "__filename", "__dirname"};
    if (napi_create_array_with_length(env, 5, &params) != napi_ok) {
      *detail_out = "failed to create compile arguments";
      return FileResult::kFailed;
    }
    for (uint32_t i = 0; i < 5; ++i) {
      napi_value param = nullptr;
      if (napi_create_string_utf8(env, kParams[i], NAPI_AUTO_LENGTH, &param) != napi_ok ||
          napi_set_element(env, params, i, param) != napi_ok) {
        *detail_out = "failed to create compile arguments";
        return FileResult::kFailed;
      }
    }
  }

  const int32_t bytecode_shape = shape == FileShape::kEsm
                                     ? unofficial_napi_bytecode_shape_module
                                     : unofficial_napi_bytecode_shape_cjs_function;
  void* bytecode = nullptr;
  bool can_parse_as_module = false;
  const napi_status status = unofficial_napi_bytecode_compile(env,
                                                              code,
                                                              filename,
                                                              bytecode_shape,
                                                              params,
                                                              nullptr,
                                                              0,
                                                              0,
                                                              &bytecode,
                                                              &can_parse_as_module);
  if (status != napi_ok || bytecode == nullptr) {
    const std::string compile_error = DescribePendingException(env);
    bool pending = false;
    napi_value ignored = nullptr;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      (void)napi_get_and_clear_last_exception(env, &ignored);
    }
    if (shape == FileShape::kCjs && can_parse_as_module) {
      // A .js file with module syntax in a commonjs scope loads as ESM at
      // runtime (detect-module); precompile it with the module shape instead.
      return PrecompileFile(env, file_path, FileShape::kEsm, detail_out);
    }
    *detail_out = compile_error;
    return FileResult::kFailed;
  }

  napi_value cache_buffer = nullptr;
  const bool serialized =
      unofficial_napi_bytecode_serialize(env, bytecode, &cache_buffer) == napi_ok && cache_buffer != nullptr;
  (void)unofficial_napi_bytecode_release(env, bytecode);
  if (!serialized) {
    *detail_out = "engine produced no cached data";
    return FileResult::kFailed;
  }

  napi_typedarray_type type = napi_uint8_array;
  size_t length = 0;
  void* data = nullptr;
  if (napi_get_typedarray_info(env, cache_buffer, &type, &length, &data, nullptr, nullptr) != napi_ok ||
      type != napi_uint8_array || data == nullptr || length == 0) {
    *detail_out = "engine produced no cached data";
    return FileResult::kFailed;
  }

  const uint32_t sidecar_flags = shape == FileShape::kEsm ? edge_bytecode_cache::kFlagEsmModuleV1
                                                          : edge_bytecode_cache::kFlagCjsFunctionV1;
  if (!edge_bytecode_cache::WriteSidecar(path_utf8, source, sidecar_flags,
                                         static_cast<const uint8_t*>(data), length)) {
    *detail_out = "failed to write sidecar (read-only or inaccessible location?)";
    return FileResult::kFailed;
  }
  return FileResult::kWritten;
}

void CollectFromDirectory(const fs::path& dir,
                          PackageTypeResolver* resolver,
                          std::vector<PrecompileEntry>* out) {
  std::error_code ec;
  fs::recursive_directory_iterator it(dir, fs::directory_options::skip_permission_denied, ec);
  if (ec) return;
  for (const auto& entry : it) {
    std::error_code entry_ec;
    if (!entry.is_regular_file(entry_ec) || entry_ec) continue;
    const fs::path& path = entry.path();
    const std::string ext = path.extension().string();
    if (ext == ".cjs") {
      out->push_back({path, FileShape::kCjs});
    } else if (ext == ".mjs") {
      out->push_back({path, FileShape::kEsm});
    } else if (ext == ".js") {
      out->push_back({path, resolver->IsModuleScope(path) ? FileShape::kEsm : FileShape::kCjs});
    }
  }
}

}  // namespace

int RunPrecompile(napi_env env, const std::vector<std::string>& paths, std::string* error_out) {
  if (!edge_bytecode_cache::Enabled()) {
    if (error_out != nullptr) {
      *error_out = "bytecode cache is disabled (EDGE_BYTECODE_CACHE or unsupported engine); "
                   "--precompile cannot proceed";
    }
    return 1;
  }

  PackageTypeResolver resolver;
  std::vector<PrecompileEntry> files;
  size_t skipped = 0;
  for (const auto& raw_path : paths) {
    const fs::path path(raw_path);
    std::error_code ec;
    if (fs::is_directory(path, ec) && !ec) {
      CollectFromDirectory(path, &resolver, &files);
      continue;
    }
    if (fs::is_regular_file(path, ec) && !ec) {
      const std::string ext = path.extension().string();
      if (ext == ".cjs") {
        files.push_back({path, FileShape::kCjs});
      } else if (ext == ".mjs") {
        files.push_back({path, FileShape::kEsm});
      } else if (ext == ".js") {
        files.push_back({path, resolver.IsModuleScope(path) ? FileShape::kEsm : FileShape::kCjs});
      } else {
        std::fprintf(stderr, "edge --precompile: skipping %s (not a .js/.cjs/.mjs file)\n",
                     path.string().c_str());
        ++skipped;
      }
      continue;
    }
    if (error_out != nullptr) {
      *error_out = "no such file or directory: " + raw_path;
    }
    return 1;
  }
  std::sort(files.begin(), files.end());
  files.erase(std::unique(files.begin(), files.end()), files.end());

  size_t written = 0;
  size_t failed = 0;
  for (const auto& file : files) {
    std::error_code abs_ec;
    fs::path absolute = fs::absolute(file.path, abs_ec);
    if (abs_ec) absolute = file.path;
    std::string detail;
    switch (PrecompileFile(env, absolute.lexically_normal(), file.shape, &detail)) {
      case FileResult::kWritten:
        ++written;
        break;
      case FileResult::kSkipped:
        std::fprintf(stderr, "edge --precompile: skipping %s: %s\n",
                     file.path.string().c_str(), detail.c_str());
        ++skipped;
        break;
      case FileResult::kFailed:
        std::fprintf(stderr, "edge --precompile: error in %s: %s\n",
                     file.path.string().c_str(), detail.c_str());
        ++failed;
        break;
    }
  }

  std::fprintf(stderr,
               "edge --precompile: wrote %zu sidecar(s) (__edgecache__/*.jsc), skipped %zu, "
               "%zu error(s)\n",
               written, skipped, failed);
  return failed > 0 ? 1 : 0;
}

}  // namespace edge_precompile
