#ifndef EDGE_TASK_QUEUE_H_
#define EDGE_TASK_QUEUE_H_

#include "node_api.h"

napi_value EdgeGetOrCreateTaskQueueBinding(napi_env env);
napi_status EdgeRunTaskQueueTickCallback(napi_env env, bool* called);
bool EdgeGetTaskQueueFlags(napi_env env, bool* has_tick_scheduled, bool* has_rejection_to_warn);

#endif  // EDGE_TASK_QUEUE_H_
