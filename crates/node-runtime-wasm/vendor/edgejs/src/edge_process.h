#ifndef EDGE_PROCESS_H_
#define EDGE_PROCESS_H_

#include <string>
#include <vector>

#if defined(ENABLE_TRACING)
#include <cstdint>
#endif

#include "node_api.h"

napi_status EdgeInstallProcessObject(napi_env env,
                                      const std::string& current_script_path,
                                      const std::vector<std::string>& exec_argv,
                                      const std::vector<std::string>& script_argv,
                                      const std::string& process_title);
std::string EdgeGetProcessExecPath();
void EdgeSetProcessArgv0(const std::string& argv0);
#if defined(ENABLE_TRACING)
uint64_t EdgeGetProcessStartTimeNanoseconds();
#endif

napi_value EdgeGetProcessMethodsBinding(napi_env env);
napi_value EdgeGetReportBinding(napi_env env);
bool EdgeWriteReportForUncaughtException(napi_env env, napi_value exception);

#endif  // EDGE_PROCESS_H_
