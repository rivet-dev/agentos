#ifndef EDGE_CLI_H_
#define EDGE_CLI_H_

#include <string>

inline constexpr const char kEdgeInternalEnvCliDispatchFlag[] = "--edge-internal-env-cli";

void EdgeInitializeCliProcess();
int EdgeRunCli(int argc, const char* const* argv, std::string* error_out);
int EdgeRunEnvCli(int argc, const char* const* argv, std::string* error_out);
int EdgeRunCliScript(const char* script_path, std::string* error_out);

#endif  // EDGE_CLI_H_
