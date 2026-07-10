#ifndef EDGE_COMPAT_EXEC_H_
#define EDGE_COMPAT_EXEC_H_

#include <string>
#include <string_view>
#include <vector>

bool EdgeShouldWrapCompatCommand(std::string_view command);
int EdgeRunCompatCommand(int argc, const char* const* argv, std::string* error_out);
int EdgeRunSafeModeCommand(const std::vector<std::string>& forwarded_args,
                           std::string_view wasmer_bin,
                           std::string_view wasmer_package,
                           std::string* error_out);

#endif  // EDGE_COMPAT_EXEC_H_
