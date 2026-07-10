#pragma once

#include <filesystem>
#include <string>
#include <vector>

namespace builtin_catalog {

struct BuiltinCategories {
  std::vector<std::string> can_be_required;
  std::vector<std::string> cannot_be_required;
};

const std::filesystem::path& NodeLibRoot();
const std::filesystem::path& NodeDepsRoot();

bool ResolveBuiltinId(const std::string& specifier, std::filesystem::path* out_path);
bool TryGetBuiltinIdForPath(const std::filesystem::path& resolved_path, std::string* out_id);
bool TryReadBuiltinSource(const std::filesystem::path& resolved_path, std::string* out_source);
bool TryReadBuiltinSource(const std::string& specifier, std::string* out_source);

const std::vector<std::string>& AllBuiltinIds();
const BuiltinCategories& GetBuiltinCategories();

}  // namespace builtin_catalog
