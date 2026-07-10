#ifndef EDGE_OPTION_HELPERS_H_
#define EDGE_OPTION_HELPERS_H_

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "../../deps/simdjson/simdjson.h"

namespace edge_options {

namespace fs = std::filesystem;

struct ParsedDotenvResult {
  std::unordered_map<std::string, std::string> values;
  bool ok = true;
  std::string error;
};

struct EffectiveCliState {
  std::vector<std::string> node_options_tokens;
  std::vector<std::string> config_tokens;
  std::vector<std::string> effective_tokens;
  std::vector<std::string> warnings;
  std::unordered_map<std::string, std::string> env_updates;
  bool ok = true;
  std::string error;
};

inline std::string TrimAsciiWhitespace(std::string_view input) {
  size_t start = 0;
  while (start < input.size() &&
         std::isspace(static_cast<unsigned char>(input[start])) != 0) {
    start++;
  }
  size_t end = input.size();
  while (end > start &&
         std::isspace(static_cast<unsigned char>(input[end - 1])) != 0) {
    end--;
  }
  return std::string(input.substr(start, end - start));
}

inline std::string MaybeUnescapeLeadingDashOptionValue(const std::string& value) {
  if (value.size() >= 2 && value[0] == '\\' && value[1] == '-') {
    return value.substr(1);
  }
  return value;
}

inline bool ReadFileText(const fs::path& path, std::string* out) {
  if (out == nullptr) return false;
  std::ifstream in(path, std::ios::in | std::ios::binary);
  if (!in.is_open()) return false;
  std::ostringstream ss;
  ss << in.rdbuf();
  *out = ss.str();
  return true;
}

inline std::optional<fs::path> TryGetCurrentPath() {
  std::error_code ec;
  fs::path cwd = fs::current_path(ec);
  if (ec) return std::nullopt;
  return cwd;
}

inline std::vector<std::string> ParseNodeOptionsString(const std::string& node_options,
                                                       std::string* error_out = nullptr) {
  if (error_out != nullptr) error_out->clear();
  std::vector<std::string> out;
  bool in_string = false;
  bool start_new_arg = true;
  for (size_t i = 0; i < node_options.size(); ++i) {
    char ch = node_options[i];
    if (ch == '\\' && in_string) {
      if (i + 1 >= node_options.size()) {
        if (error_out != nullptr) *error_out = "invalid value for NODE_OPTIONS (invalid escape)";
        return {};
      }
      ch = node_options[++i];
    } else if (ch == ' ' && !in_string) {
      start_new_arg = true;
      continue;
    } else if (ch == '"') {
      in_string = !in_string;
      continue;
    }

    if (start_new_arg) {
      out.emplace_back(1, ch);
      start_new_arg = false;
    } else {
      out.back().push_back(ch);
    }
  }
  if (in_string) {
    if (error_out != nullptr) *error_out = "invalid value for NODE_OPTIONS (unterminated string)";
    return {};
  }
  return out;
}

inline bool IsDotenvCommentBoundary(const std::string& value, size_t index) {
  if (index >= value.size() || value[index] != '#') return false;
  if (index == 0) return true;
  return std::isspace(static_cast<unsigned char>(value[index - 1])) != 0;
}

inline std::string UnescapeDoubleQuotedValue(const std::string& value) {
  std::string out;
  out.reserve(value.size());
  for (size_t i = 0; i < value.size(); ++i) {
    const char ch = value[i];
    if (ch != '\\' || i + 1 >= value.size()) {
      out.push_back(ch);
      continue;
    }
    switch (value[++i]) {
      case 'n':
        out.push_back('\n');
        break;
      case 'r':
        out.push_back('\r');
        break;
      case 't':
        out.push_back('\t');
        break;
      case '\\':
        out.push_back('\\');
        break;
      case '"':
        out.push_back('"');
        break;
      default:
        out.push_back(value[i]);
        break;
    }
  }
  return out;
}

inline ParsedDotenvResult ParseDotenvContent(const std::string& content) {
  ParsedDotenvResult result;
  size_t index = 0;

  auto skip_spaces = [&](size_t* pos) {
    while (*pos < content.size()) {
      const char ch = content[*pos];
      if (ch == ' ' || ch == '\t' || ch == '\r') {
        (*pos)++;
        continue;
      }
      break;
    }
  };

  while (index < content.size()) {
    while (index < content.size() &&
           (content[index] == '\n' || content[index] == '\r')) {
      index++;
    }
    if (index >= content.size()) break;

    size_t line_start = index;
    skip_spaces(&index);
    if (index >= content.size()) break;
    if (content[index] == '#') {
      while (index < content.size() && content[index] != '\n') index++;
      continue;
    }

    if (content.compare(index, 6, "export") == 0) {
      const size_t after = index + 6;
      if (after >= content.size() ||
          std::isspace(static_cast<unsigned char>(content[after])) != 0) {
        index = after;
        skip_spaces(&index);
      }
    }

    const size_t key_start = index;
    while (index < content.size()) {
      const char ch = content[index];
      if (ch == '=' || ch == '\n') break;
      index++;
    }
    if (index >= content.size() || content[index] == '\n') {
      index = line_start;
      while (index < content.size() && content[index] != '\n') index++;
      continue;
    }

    std::string key = TrimAsciiWhitespace(
        std::string_view(content).substr(key_start, index - key_start));
    index++;  // '='
    skip_spaces(&index);
    if (key.empty()) {
      while (index < content.size() && content[index] != '\n') index++;
      continue;
    }

    std::string value;
    if (index < content.size() &&
        (content[index] == '"' || content[index] == '\'' || content[index] == '`')) {
      const char quote = content[index++];
      const size_t value_start = index;
      while (index < content.size() && content[index] != quote) index++;
      if (index >= content.size()) {
        break;
      }
      value.assign(content.data() + value_start, index - value_start);
      index++;  // closing quote
      if (quote == '"') value = UnescapeDoubleQuotedValue(value);
      while (index < content.size() && content[index] != '\n') index++;
    } else {
      const size_t value_start = index;
      while (index < content.size() && content[index] != '\n') index++;
      std::string raw(content.data() + value_start, index - value_start);
      size_t comment_pos = std::string::npos;
      for (size_t i = 0; i < raw.size(); ++i) {
        if (IsDotenvCommentBoundary(raw, i)) {
          comment_pos = i;
          break;
        }
      }
      if (comment_pos != std::string::npos) raw.resize(comment_pos);
      value = TrimAsciiWhitespace(raw);
    }

    result.values[key] = value;
  }

  return result;
}

inline void CollectEnvFileSpecs(const std::vector<std::string>& raw_exec_argv,
                                std::vector<std::pair<fs::path, bool>>* out_specs) {
  if (out_specs == nullptr) return;
  out_specs->clear();
  bool needs_cwd = false;
  for (const auto& token : raw_exec_argv) {
    if (token == "--env-file" ||
        token == "--env-file-if-exists" ||
        token.rfind("--env-file=", 0) == 0 ||
        token.rfind("--env-file-if-exists=", 0) == 0) {
      needs_cwd = true;
      break;
    }
  }
  if (!needs_cwd) return;
  const std::optional<fs::path> cwd = TryGetCurrentPath();

  for (size_t i = 0; i < raw_exec_argv.size(); ++i) {
    const std::string& token = raw_exec_argv[i];
    auto push_spec = [&](const std::string& raw_path, bool optional) {
      fs::path path(raw_path);
      if (path.is_relative() && cwd.has_value()) path = *cwd / path;
      out_specs->push_back({path.lexically_normal(), optional});
    };

    if (token.rfind("--env-file=", 0) == 0) {
      push_spec(token.substr(sizeof("--env-file=") - 1), false);
      continue;
    }
    if (token == "--env-file" && i + 1 < raw_exec_argv.size()) {
      push_spec(raw_exec_argv[++i], false);
      continue;
    }
    if (token.rfind("--env-file-if-exists=", 0) == 0) {
      push_spec(token.substr(sizeof("--env-file-if-exists=") - 1), true);
      continue;
    }
    if (token == "--env-file-if-exists" && i + 1 < raw_exec_argv.size()) {
      push_spec(raw_exec_argv[++i], true);
      continue;
    }
  }
}

inline void ApplyEnvFiles(const std::vector<std::pair<fs::path, bool>>& specs,
                          std::unordered_map<std::string, std::string>* env_updates,
                          std::vector<std::string>* warnings_out = nullptr,
                          std::string* error_out = nullptr) {
  if (error_out != nullptr) error_out->clear();
  if (env_updates == nullptr) return;
  std::unordered_set<std::string> original_env_keys;
  for (const auto& [path, optional] : specs) {
    std::error_code exists_error;
    const bool exists = fs::exists(path, exists_error);
    if (!exists) {
      if (optional && !exists_error) {
        if (warnings_out != nullptr) {
          warnings_out->push_back(
              "Warning: ignoring missing environment file: " + path.string());
        }
        continue;
      }
      if (error_out != nullptr) {
        *error_out = "Failed to read env file: " + path.string();
      }
      return;
    }

    std::string content;
    if (!ReadFileText(path, &content)) {
      if (error_out != nullptr) {
        *error_out = "Failed to read env file: " + path.string();
      }
      return;
    }
    ParsedDotenvResult parsed = ParseDotenvContent(content);
    if (!parsed.ok) {
      if (error_out != nullptr) {
        *error_out = parsed.error + ": " + path.string();
      }
      return;
    }
    for (const auto& [key, value] : parsed.values) {
      if (original_env_keys.find(key) == original_env_keys.end()) {
        if (std::getenv(key.c_str()) != nullptr) {
          original_env_keys.emplace(key);
          continue;
        }
      } else {
        continue;
      }
      (*env_updates)[key] = value;
    }
  }
}

inline void CollectConfigFileSpecs(const std::vector<std::string>& raw_exec_argv,
                                   std::vector<fs::path>* out_paths) {
  if (out_paths == nullptr) return;
  out_paths->clear();
  bool needs_cwd = false;
  for (const auto& token : raw_exec_argv) {
    if (token == "--experimental-config-file" ||
        token.rfind("--experimental-config-file=", 0) == 0) {
      needs_cwd = true;
      break;
    }
  }
  if (!needs_cwd) return;
  const std::optional<fs::path> cwd = TryGetCurrentPath();
  for (size_t i = 0; i < raw_exec_argv.size(); ++i) {
    const std::string& token = raw_exec_argv[i];
    if (token.rfind("--experimental-config-file=", 0) == 0) {
      fs::path path(token.substr(sizeof("--experimental-config-file=") - 1));
      if (path.is_relative() && cwd.has_value()) path = *cwd / path;
      out_paths->push_back(path.lexically_normal());
      continue;
    }
    if (token == "--experimental-config-file" && i + 1 < raw_exec_argv.size()) {
      fs::path path(raw_exec_argv[++i]);
      if (path.is_relative() && cwd.has_value()) path = *cwd / path;
      out_paths->push_back(path.lexically_normal());
    }
  }
}

inline void AppendJsonValueAsFlags(const std::string& key,
                                   simdjson::ondemand::value value,
                                   std::vector<std::string>* out) {
  if (out == nullptr || key.empty()) return;
  simdjson::ondemand::json_type type;
  if (value.type().get(type) != simdjson::SUCCESS) return;
  const std::string flag = "--" + key;

  switch (type) {
    case simdjson::ondemand::json_type::boolean: {
      bool enabled = false;
      if (value.get_bool().get(enabled) == simdjson::SUCCESS) {
        out->push_back(enabled ? flag : "--no-" + key);
      }
      break;
    }
    case simdjson::ondemand::json_type::number: {
      double num = 0;
      if (value.get_double().get(num) == simdjson::SUCCESS) {
        std::ostringstream ss;
        if (num == static_cast<double>(static_cast<int64_t>(num))) {
          ss << static_cast<int64_t>(num);
        } else {
          ss << num;
        }
        out->push_back(flag + "=" + ss.str());
      }
      break;
    }
    case simdjson::ondemand::json_type::string: {
      std::string_view str;
      if (value.get_string().get(str) == simdjson::SUCCESS) {
        out->push_back(flag + "=" + std::string(str));
      }
      break;
    }
    case simdjson::ondemand::json_type::array: {
      simdjson::ondemand::array array;
      if (value.get_array().get(array) != simdjson::SUCCESS) break;
      for (auto item : array) {
        AppendJsonValueAsFlags(key, item.value(), out);
      }
      break;
    }
    default:
      break;
  }
}

inline std::vector<std::string> ParseConfigFileFlags(const fs::path& path,
                                                     std::string* error_out = nullptr) {
  if (error_out != nullptr) error_out->clear();
  std::string content;
  if (!ReadFileText(path, &content)) {
    if (error_out != nullptr) *error_out = "Failed to read config file: " + path.string();
    return {};
  }

  simdjson::ondemand::parser parser;
  simdjson::padded_string padded(content);
  simdjson::ondemand::document document;
  simdjson::ondemand::object root;
  if (parser.iterate(padded).get(document) != simdjson::SUCCESS ||
      document.get_object().get(root) != simdjson::SUCCESS) {
    if (error_out != nullptr) *error_out = "Failed to parse config file: " + path.string();
    return {};
  }

  std::vector<std::string> out;
  for (auto field : root) {
    std::string_view raw_key;
    simdjson::ondemand::value value;
    if (field.unescaped_key().get(raw_key) != simdjson::SUCCESS ||
        field.value().get(value) != simdjson::SUCCESS) {
      continue;
    }
    std::string key(raw_key);
    if (key == "$schema") continue;
    simdjson::ondemand::object section;
    if (value.get_object().get(section) != simdjson::SUCCESS) continue;
    for (auto opt_field : section) {
      std::string_view opt_raw_key;
      simdjson::ondemand::value opt_value;
      if (opt_field.unescaped_key().get(opt_raw_key) != simdjson::SUCCESS ||
          opt_field.value().get(opt_value) != simdjson::SUCCESS) {
        continue;
      }
      std::string opt(opt_raw_key);
      AppendJsonValueAsFlags(opt, opt_value, &out);
    }
  }
  return out;
}

inline void ApplyImpliedCliFlags(std::vector<std::string>* tokens) {
  if (tokens == nullptr) return;
  bool has_transform_types = false;
  for (const auto& token : *tokens) {
    if (token == "--experimental-transform-types") {
      has_transform_types = true;
      break;
    }
  }
  if (!has_transform_types) return;
  tokens->push_back("--strip-types");
  tokens->push_back("--enable-source-maps");
}

inline EffectiveCliState BuildEffectiveCliState(const std::vector<std::string>& raw_exec_argv) {
  EffectiveCliState state;

  std::vector<std::pair<fs::path, bool>> env_specs;
  CollectEnvFileSpecs(raw_exec_argv, &env_specs);
  ApplyEnvFiles(env_specs, &state.env_updates, &state.warnings, &state.error);
  if (!state.error.empty()) {
    state.ok = false;
    return state;
  }

  const char* env_node_options = std::getenv("NODE_OPTIONS");
  auto node_options_source = (env_node_options != nullptr)
                                 ? std::string(env_node_options)
                                 : std::string();
  if (env_node_options == nullptr) {
    auto it = state.env_updates.find("NODE_OPTIONS");
    if (it != state.env_updates.end()) node_options_source = it->second;
  }
  if (!node_options_source.empty()) {
    state.node_options_tokens = ParseNodeOptionsString(node_options_source, &state.error);
    if (!state.error.empty()) {
      state.ok = false;
      return state;
    }
  }

  std::vector<fs::path> config_paths;
  CollectConfigFileSpecs(raw_exec_argv, &config_paths);
  for (const fs::path& path : config_paths) {
    std::string error;
    std::vector<std::string> flags = ParseConfigFileFlags(path, &error);
    if (!error.empty()) {
      state.ok = false;
      state.error = error;
      return state;
    }
    state.config_tokens.insert(state.config_tokens.end(), flags.begin(), flags.end());
  }

  state.effective_tokens.reserve(state.node_options_tokens.size() +
                                 state.config_tokens.size() +
                                 raw_exec_argv.size());
  state.effective_tokens.insert(state.effective_tokens.end(),
                                state.node_options_tokens.begin(),
                                state.node_options_tokens.end());
  state.effective_tokens.insert(state.effective_tokens.end(),
                                state.config_tokens.begin(),
                                state.config_tokens.end());
  state.effective_tokens.insert(state.effective_tokens.end(),
                                raw_exec_argv.begin(),
                                raw_exec_argv.end());
  ApplyImpliedCliFlags(&state.effective_tokens);
  return state;
}

}  // namespace edge_options

#endif  // EDGE_OPTION_HELPERS_H_
