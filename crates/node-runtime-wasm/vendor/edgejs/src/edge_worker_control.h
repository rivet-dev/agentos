#ifndef EDGE_WORKER_CONTROL_H_
#define EDGE_WORKER_CONTROL_H_

#include <string>
#include <vector>

#include "node_api.h"

struct EdgeWorkerReportEntry {
  int32_t thread_id = 0;
  std::string thread_name;
  std::string json;
};

void EdgeWorkerStopAllForEnv(napi_env env);
std::vector<EdgeWorkerReportEntry> EdgeWorkerCollectReports(napi_env env);

#endif  // EDGE_WORKER_CONTROL_H_
