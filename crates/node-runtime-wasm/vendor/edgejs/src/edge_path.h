#ifndef EDGE_PATH_H_
#define EDGE_PATH_H_

#include <initializer_list>
#include <string>
#include <string_view>
#include <vector>

namespace edge_path {

bool IsPathSeparator(char c) noexcept;
std::string GetCurrentWorkingDirectory();

std::string NormalizeString(std::string_view path,
                            bool allow_above_root,
                            std::string_view separator);

std::string PathResolve(std::string_view cwd,
                        const std::vector<std::string_view>& paths);
std::string PathResolve(const std::vector<std::string_view>& paths);
std::string PathResolve(std::initializer_list<std::string_view> paths);

bool IsAbsoluteFilePath(std::string_view path);
std::string NormalizeFileURLOrPath(std::string_view path);
std::string ToNamespacedPath(std::string_view path);
std::string FromNamespacedPath(std::string_view path);

#ifdef _WIN32
bool IsWindowsDeviceRoot(char c) noexcept;
bool IsWindowsDriveLetter(std::string_view path) noexcept;
#endif

}  // namespace edge_path

#endif  // EDGE_PATH_H_
