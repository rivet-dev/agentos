#ifndef EDGE_BINDING_REGISTRY_BINDING_LIST_H_
#define EDGE_BINDING_REGISTRY_BINDING_LIST_H_

// Single manifest for the compiled-in internal binding surface.
// Keep entries sorted by the JS-visible binding name.
#define EDGE_BINDING_REGISTRY_LIST(V)          \
  V(async_context_frame, internal_binding::InitAsyncContextFrame) \
  V(async_wrap, internal_binding::InitAsyncWrap) \
  V(blob, internal_binding::InitBlob)          \
  V(block_list, internal_binding::InitBlockList) \
  V(buffer, EdgeInstallBufferBinding)          \
  V(builtins, EdgeInstallBuiltinsBinding)      \
  V(cares_wrap, EdgeInstallCaresWrapBinding)   \
  V(config, internal_binding::InitConfig)      \
  V(constants, internal_binding::InitConstants) \
  V(contextify, EdgeInstallContextifyBinding)  \
  V(credentials, internal_binding::InitCredentials) \
  V(crypto, internal_binding::InitCrypto)      \
  V(encoding_binding, EdgeInstallEncodingBinding) \
  V(errors, EdgeGetOrCreateErrorsBinding)      \
  V(fs, internal_binding::InitFs)              \
  V(fs_dir, internal_binding::InitFsDir)       \
  V(fs_event_wrap, internal_binding::InitFsEventWrap) \
  V(heap_utils, internal_binding::InitHeapUtils) \
  V(http2, internal_binding::InitHttp2)        \
  V(http_parser, EdgeInstallHttpParserBinding) \
  V(icu, internal_binding::InitIcu)            \
  V(inspector, internal_binding::InitInspector) \
  V(internal_only_v8, internal_binding::InitInternalOnlyV8) \
  V(js_stream, EdgeInstallJsStreamBinding)     \
  V(js_udp_wrap, EdgeInstallJsUdpWrapBinding)  \
  V(messaging, internal_binding::InitMessaging) \
  V(mksnapshot, internal_binding::InitMksnapshot) \
  V(module_wrap, internal_binding::InitModuleWrap) \
  V(modules, EdgeInstallModulesBinding)        \
  V(options, EdgeInstallOptionsBinding)        \
  V(os, EdgeInstallOsBinding)                  \
  V(performance, internal_binding::InitPerformance) \
  V(permission, internal_binding::InitPermission) \
  V(pipe_wrap, EdgeInstallPipeWrapBinding)     \
  V(process_methods, EdgeGetProcessMethodsBinding) \
  V(process_wrap, EdgeInstallProcessWrapBinding) \
  V(report, EdgeGetReportBinding)              \
  V(sea, internal_binding::InitSea)            \
  V(serdes, internal_binding::InitSerdes)      \
  V(signal_wrap, EdgeInstallSignalWrapBinding) \
  V(spawn_sync, EdgeInstallSpawnSyncBinding)   \
  V(stream_pipe, internal_binding::InitStreamPipe) \
  V(stream_wrap, EdgeInstallStreamWrapBinding) \
  V(string_decoder, EdgeInstallStringDecoderBinding) \
  V(symbols, internal_binding::InitSymbols)    \
  V(task_queue, EdgeGetOrCreateTaskQueueBinding) \
  V(tcp_wrap, EdgeInstallTcpWrapBinding)       \
  V(timers, EdgeInstallTimersHostBinding)      \
  V(tls_wrap, EdgeInstallTlsWrapBinding)       \
  V(trace_events, EdgeInstallTraceEventsBinding) \
  V(tty_wrap, EdgeInstallTtyWrapBinding)       \
  V(types, EdgeGetTypesBinding)                \
  V(udp_wrap, EdgeInstallUdpWrapBinding)       \
  V(undici, internal_binding::InitUndici)      \
  V(url, EdgeInstallUrlBinding)                \
  V(url_pattern, EdgeInstallUrlPatternBinding) \
  V(util, internal_binding::InitUtil)          \
  V(uv, EdgeInstallUvBinding)                  \
  V(v8, internal_binding::InitV8)              \
  V(wasm_web_api, internal_binding::InitWasmWebApi) \
  V(watchdog, internal_binding::InitWatchdog)  \
  V(worker, internal_binding::InitWorker)      \
  V(zlib, internal_binding::InitZlib)

#endif  // EDGE_BINDING_REGISTRY_BINDING_LIST_H_
