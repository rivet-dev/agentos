#include "edge_path.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdlib>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "ada.h"
#include "uv.h"

namespace edge_path {

namespace {

char ToLowerAscii(char c) {
  return static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
}

std::string ToLowerAscii(std::string_view input) {
  std::string out(input);
  std::transform(out.begin(), out.end(), out.begin(), [](char c) { return ToLowerAscii(c); });
  return out;
}

std::optional<std::string> FileURLToPath(std::string_view input) {
  auto parsed = ada::parse<ada::url_aggregator>(std::string(input));
  if (!parsed || parsed->type != ada::scheme::FILE) return std::nullopt;

  const ada::url_aggregator& file_url = parsed.value();
  std::string_view pathname = file_url.get_pathname();

#ifdef _WIN32
  size_t first_percent = std::string::npos;
  std::string pathname_escaped_slash;
  pathname_escaped_slash.reserve(pathname.size());

  for (size_t i = 0; i < pathname.size(); ++i) {
    const char ch = pathname[i];
    pathname_escaped_slash.push_back(ch == '/' ? '\\' : ch);
    if (ch != '%') continue;
    if (first_percent == std::string::npos) first_percent = i;
    if ((i + 2) >= pathname.size()) continue;
    const char third = static_cast<char>(pathname[i + 2] | 0x20);
    const bool is_backslash = pathname[i + 1] == '2' && third == 'f';
    const bool is_forward_slash = pathname[i + 1] == '5' && third == 'c';
    if (is_backslash || is_forward_slash) return std::nullopt;
  }

  std::string_view hostname = file_url.get_hostname();
  std::string decoded_pathname =
      ada::unicode::percent_decode(std::string_view(pathname_escaped_slash), first_percent);

  if (!hostname.empty()) {
    return "\\\\" + ada::idna::to_unicode(hostname) + decoded_pathname;
  }

  if (decoded_pathname.size() < 3) return std::nullopt;
  const char letter = static_cast<char>(decoded_pathname[1] | 0x20);
  const char sep = decoded_pathname[2];
  if (letter < 'a' || letter > 'z' || sep != ':') return std::nullopt;
  return decoded_pathname.substr(1);
#else
  std::string_view hostname = file_url.get_hostname();
  if (!hostname.empty()) return std::nullopt;

  size_t first_percent = std::string::npos;
  for (size_t i = 0; (i + 2) < pathname.size(); ++i) {
    if (pathname[i] != '%') continue;
    if (first_percent == std::string::npos) first_percent = i;
    if (pathname[i + 1] == '2' && static_cast<char>(pathname[i + 2] | 0x20) == 'f') {
      return std::nullopt;
    }
  }

  std::string decoded = ada::unicode::percent_decode(pathname, first_percent);
  if (decoded.empty() || decoded.front() != '/') {
    decoded.insert(decoded.begin(), '/');
  }
  return decoded;
#endif
}

}  // namespace

#ifdef _WIN32
bool IsPathSeparator(char c) noexcept {
  return c == '\\' || c == '/';
}

bool IsWindowsDeviceRoot(char c) noexcept {
  const unsigned char ch = static_cast<unsigned char>(c);
  return (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z');
}

bool IsWindowsDriveLetter(std::string_view path) noexcept {
  return path.size() > 2 && IsWindowsDeviceRoot(path[0]) &&
         path[1] == ':' && (path[2] == '/' || path[2] == '\\');
}
#else
bool IsPathSeparator(char c) noexcept {
  return c == '/';
}
#endif

std::string GetCurrentWorkingDirectory() {
  size_t cwd_len = 256;
  for (;;) {
    std::string cwd(cwd_len, '\0');
    const int rc = uv_cwd(cwd.data(), &cwd_len);
    if (rc == 0) {
      cwd.resize(cwd_len);
      return cwd;
    }
    if (rc != UV_ENOBUFS) return ".";
    cwd_len += 1;
  }
}

std::string NormalizeString(std::string_view path,
                            bool allow_above_root,
                            std::string_view separator) {
  std::string result;
  int last_segment_length = 0;
  int last_slash = -1;
  int dots = 0;
  char code = 0;

  for (size_t i = 0; i <= path.size(); ++i) {
    if (i < path.size()) {
      code = path[i];
    } else if (IsPathSeparator(code)) {
      break;
    } else {
      code = '/';
    }

    if (IsPathSeparator(code)) {
      if (last_slash == static_cast<int>(i - 1) || dots == 1) {
        // no-op
      } else if (dots == 2) {
        const int len = static_cast<int>(result.length());
        if (len < 2 || last_segment_length != 2 || result[len - 1] != '.' || result[len - 2] != '.') {
          if (len > 2) {
            const size_t last_slash_index = result.find_last_of(separator);
            if (last_slash_index == std::string::npos) {
              result.clear();
              last_segment_length = 0;
            } else {
              result = result.substr(0, last_slash_index);
              last_segment_length =
                  static_cast<int>(result.length()) - 1 -
                  static_cast<int>(result.find_last_of(separator));
            }
            last_slash = static_cast<int>(i);
            dots = 0;
            continue;
          } else if (len != 0) {
            result.clear();
            last_segment_length = 0;
            last_slash = static_cast<int>(i);
            dots = 0;
            continue;
          }
        }

        if (allow_above_root) {
          result += result.empty() ? ".." : std::string(separator) + "..";
          last_segment_length = 2;
        }
      } else {
        const std::string_view segment = path.substr(last_slash + 1, i - (last_slash + 1));
        if (!result.empty()) {
          result += std::string(separator);
          result += segment;
        } else {
          result.assign(segment);
        }
        last_segment_length = static_cast<int>(i - last_slash - 1);
      }
      last_slash = static_cast<int>(i);
      dots = 0;
    } else if (code == '.' && dots != -1) {
      ++dots;
    } else {
      dots = -1;
    }
  }

  return result;
}

#ifdef _WIN32
std::string PathResolve(std::string_view cwd,
                        const std::vector<std::string_view>& paths) {
  std::string resolved_device;
  std::string resolved_tail;
  bool resolved_absolute = false;

  for (int i = static_cast<int>(paths.size()) - 1; i >= -1; --i) {
    std::string path;
    if (i >= 0) {
      path = std::string(paths[static_cast<size_t>(i)]);
    } else if (resolved_device.empty()) {
      path = std::string(cwd);
    } else {
      const std::string envvar = "=" + resolved_device;
      const char* resolved_device_cwd = std::getenv(envvar.c_str());
      path = (resolved_device_cwd == nullptr || resolved_device_cwd[0] == '\0')
                 ? std::string(cwd)
                 : std::string(resolved_device_cwd);
      if (path.empty() ||
          (path.size() > 2 &&
           ToLowerAscii(path.substr(0, 2)) != ToLowerAscii(resolved_device) &&
           path[2] == '/')) {
        path = resolved_device + "\\";
      }
    }

    if (path.empty()) continue;

    const size_t len = path.length();
    int root_end = 0;
    std::string device;
    bool is_absolute = false;
    const char code = path[0];

    if (len == 1) {
      if (IsPathSeparator(code)) {
        root_end = 1;
        is_absolute = true;
      }
    } else if (IsPathSeparator(code)) {
      is_absolute = true;
      if (IsPathSeparator(path[1])) {
        size_t j = 2;
        size_t last = j;
        while (j < len && !IsPathSeparator(path[j])) ++j;
        if (j < len && j != last) {
          const std::string first_part = path.substr(last, j - last);
          last = j;
          while (j < len && IsPathSeparator(path[j])) ++j;
          if (j < len && j != last) {
            last = j;
            while (j < len && !IsPathSeparator(path[j])) ++j;
            if (j == len || j != last) {
              if (first_part != "." && first_part != "?") {
                device = "\\\\" + first_part + "\\" + path.substr(last, j - last);
                root_end = static_cast<int>(j);
              } else {
                device = "\\\\" + first_part;
                root_end = 4;
              }
            }
          }
        }
      }
    } else if (len > 1 && IsWindowsDeviceRoot(code) && path[1] == ':') {
      device = path.substr(0, 2);
      root_end = 2;
      if (len > 2 && IsPathSeparator(path[2])) {
        is_absolute = true;
        root_end = 3;
      }
    }

    if (!device.empty()) {
      if (!resolved_device.empty()) {
        if (ToLowerAscii(device) != ToLowerAscii(resolved_device)) continue;
      } else {
        resolved_device = device;
      }
    }

    if (resolved_absolute) {
      if (!resolved_device.empty()) break;
    } else {
      resolved_tail = path.substr(static_cast<size_t>(root_end)) + "\\" + resolved_tail;
      resolved_absolute = is_absolute;
      if (is_absolute && !resolved_device.empty()) break;
    }
  }

  resolved_tail = NormalizeString(resolved_tail, !resolved_absolute, "\\");
  if (resolved_absolute) return resolved_device + "\\" + resolved_tail;
  if (!resolved_device.empty() || !resolved_tail.empty()) return resolved_device + resolved_tail;
  return ".";
}
#else
std::string PathResolve(std::string_view cwd,
                        const std::vector<std::string_view>& paths) {
  std::string resolved_path;
  bool resolved_absolute = false;

  for (int i = static_cast<int>(paths.size()) - 1; i >= -1 && !resolved_absolute; --i) {
    const std::string_view path = (i >= 0) ? paths[static_cast<size_t>(i)] : cwd;
    if (path.empty()) continue;

    resolved_path = std::string(path) + "/" + resolved_path;
    if (!path.empty() && path.front() == '/') {
      resolved_absolute = true;
      break;
    }
  }

  std::string normalized_path = NormalizeString(resolved_path, !resolved_absolute, "/");
  if (resolved_absolute) return "/" + normalized_path;
  if (normalized_path.empty()) return ".";
  return normalized_path;
}
#endif

std::string PathResolve(const std::vector<std::string_view>& paths) {
  return PathResolve(GetCurrentWorkingDirectory(), paths);
}

std::string PathResolve(std::initializer_list<std::string_view> paths) {
  return PathResolve(std::vector<std::string_view>(paths));
}

bool IsAbsoluteFilePath(std::string_view path) {
  if (path.rfind("file://", 0) == 0) return true;
#ifdef _WIN32
  if (!path.empty() && path[0] == '\\') return true;
  if (IsWindowsDriveLetter(path)) return true;
#endif
  return !path.empty() && path[0] == '/';
}

std::string NormalizeFileURLOrPath(std::string_view path) {
  std::string normalized(path);
  if (normalized.rfind("file://", 0) == 0) {
    std::optional<std::string> file_path = FileURLToPath(normalized);
    if (!file_path.has_value()) return std::string();
    normalized = std::move(file_path.value());
  }
  if (normalized.empty()) return std::string();
  if (IsAbsoluteFilePath(normalized)) {
    normalized = PathResolve({normalized});
  } else {
    normalized = NormalizeString(normalized, false, "/");
  }
#ifdef _WIN32
  if (IsWindowsDriveLetter(normalized)) {
    normalized[0] = ToLowerAscii(normalized[0]);
  }
  for (char& c : normalized) {
    if (c == '\\') c = '/';
  }
#endif
  return normalized;
}

std::string ToNamespacedPath(std::string_view path) {
#ifdef _WIN32
  if (path.empty()) return std::string(path);
  std::string resolved_path = PathResolve({path});
  if (resolved_path.size() <= 2) return std::string(path);

  if (resolved_path[0] == '\\' && resolved_path[1] == '\\') {
    if (resolved_path[2] != '?' && resolved_path[2] != '.') {
      return std::string(R"(\\?\UNC\)") + resolved_path.substr(2);
    }
  } else if (IsWindowsDeviceRoot(resolved_path[0]) &&
             resolved_path[1] == ':' &&
             resolved_path[2] == '\\') {
    return std::string(R"(\\?\)") + resolved_path;
  }
  return resolved_path;
#else
  return std::string(path);
#endif
}

std::string FromNamespacedPath(std::string_view path) {
#ifdef _WIN32
  std::string result(path);
  if (result.rfind(R"(\\?\UNC\)", 0) == 0) {
    result = result.substr(8);
    result.insert(0, R"(\\)");
    return result;
  }
  if (result.rfind(R"(\\?\)", 0) == 0) {
    return result.substr(4);
  }
  return result;
#else
  return std::string(path);
#endif
}

}  // namespace edge_path
