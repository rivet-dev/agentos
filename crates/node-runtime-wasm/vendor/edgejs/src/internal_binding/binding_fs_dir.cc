#include "internal_binding/binding_initializers.h"

#include <cstring>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include <uv.h>

#include "edge_active_resource.h"
#include "edge_environment.h"
#include "edge_fs.h"
#include "internal_binding/helpers.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_runtime.h"

namespace internal_binding {

namespace {

void ResetRef(napi_env env, napi_ref* ref_ptr);

struct FsDirBindingState {
  explicit FsDirBindingState(napi_env env_in) : env(env_in) {}
  ~FsDirBindingState() {
    ResetRef(env, &binding_ref);
    ResetRef(env, &dir_handle_ctor_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref dir_handle_ctor_ref = nullptr;
};

struct DirHandleWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  uv_dir_t* dir = nullptr;
  std::vector<uv_dirent_t> dirents;
  bool closing = false;
  bool closed = false;
};

enum class DirReqKind {
  kOpen,
  kRead,
  kClose,
};

struct DirReq {
  napi_env env = nullptr;
  DirReqKind kind = DirReqKind::kOpen;
  DirHandleWrap* handle = nullptr;
  napi_ref req_ref = nullptr;
  napi_ref oncomplete_ref = nullptr;
  void* active_request_token = nullptr;
  std::string encoding = "utf8";
  std::string path;
  bool req_cleaned = false;
  uv_fs_t req{};
};

FsDirBindingState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<FsDirBindingState>(env, kEdgeEnvironmentSlotFsDirBindingState);
}

FsDirBindingState& EnsureState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<FsDirBindingState>(
      env, kEdgeEnvironmentSlotFsDirBindingState);
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

const char* ResourceNameForDirReq(DirReqKind kind) {
  switch (kind) {
    case DirReqKind::kOpen:
    case DirReqKind::kRead:
    case DirReqKind::kClose:
      return "FSReqCallback";
  }
  return "FSReqCallback";
}

void CancelDirReq(void* data) {
  auto* req = static_cast<DirReq*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

napi_value GetDirReqOwner(napi_env env, void* data) {
  auto* req = static_cast<DirReq*>(data);
  return req != nullptr ? GetRefValue(env, req->req_ref) : nullptr;
}

void ResetRef(napi_env env, napi_ref* ref_ptr) {
  if (ref_ptr == nullptr || *ref_ptr == nullptr) return;
  napi_delete_reference(env, *ref_ptr);
  *ref_ptr = nullptr;
}

void SetNamedMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

void EnsureClassProperty(napi_env env,
                         napi_value binding,
                         const char* name,
                         napi_callback ctor,
                         const std::vector<napi_property_descriptor>& methods,
                         napi_ref* out_ref) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) == napi_ok && has) {
    if (out_ref != nullptr && *out_ref == nullptr) {
      napi_value existing = nullptr;
      if (napi_get_named_property(env, binding, name, &existing) == napi_ok && existing != nullptr) {
        napi_create_reference(env, existing, 1, out_ref);
      }
    }
    return;
  }

  napi_value cls = nullptr;
  if (napi_define_class(env,
                        name,
                        NAPI_AUTO_LENGTH,
                        ctor,
                        nullptr,
                        methods.size(),
                        methods.data(),
                        &cls) != napi_ok ||
      cls == nullptr) {
    return;
  }

  napi_set_named_property(env, binding, name, cls);
  if (out_ref != nullptr) napi_create_reference(env, cls, 1, out_ref);
}

bool ValueToUtf8(napi_env env, napi_value value, std::string* out) {
  if (value == nullptr || out == nullptr) return false;
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

bool ValueToPath(napi_env env, napi_value value, std::string* out) {
  if (value == nullptr || out == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, value, &data, &len) != napi_ok || data == nullptr) return false;
    out->assign(static_cast<const char*>(data), len);
    return true;
  }

  return ValueToUtf8(env, value, out);
}

std::string GetEncoding(napi_env env, napi_value value) {
  std::string encoding = "utf8";
  if (value == nullptr || IsUndefined(env, value)) return encoding;
  if (!ValueToUtf8(env, value, &encoding)) return "utf8";
  return encoding;
}

void HoldDirHandleRef(DirHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr) return;
  uint32_t ref_count = 0;
  (void)napi_reference_ref(wrap->env, wrap->wrapper_ref, &ref_count);
}

void ReleaseDirHandleRef(DirHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr) return;
  uint32_t ref_count = 0;
  (void)napi_reference_unref(wrap->env, wrap->wrapper_ref, &ref_count);
}

napi_value CreateUvExceptionValue(napi_env env, int errorno, const char* syscall, const char* path = nullptr) {
  const char* code = uv_err_name(errorno);
  const char* message = uv_strerror(errorno);
  std::string full_message;
  full_message.append(code != nullptr ? code : "UV_UNKNOWN");
  full_message.append(": ");
  full_message.append(message != nullptr ? message : "Unknown system error");
  if (syscall != nullptr && *syscall != '\0') {
    full_message.append(", ");
    full_message.append(syscall);
    if (path != nullptr && *path != '\0') {
      full_message.append(" '");
      full_message.append(path);
      full_message.push_back('\'');
    }
  }

  napi_value message_value = nullptr;
  napi_value error = nullptr;
  napi_create_string_utf8(env, full_message.c_str(), NAPI_AUTO_LENGTH, &message_value);
  napi_create_error(env, nullptr, message_value, &error);
  if (error == nullptr) return Undefined(env);

  napi_value errno_value = nullptr;
  napi_create_int32(env, errorno, &errno_value);
  napi_set_named_property(env, error, "errno", errno_value);

  napi_value code_value = nullptr;
  napi_create_string_utf8(env, code != nullptr ? code : "UV_UNKNOWN", NAPI_AUTO_LENGTH, &code_value);
  napi_set_named_property(env, error, "code", code_value);

  if (syscall != nullptr && *syscall != '\0') {
    napi_value syscall_value = nullptr;
    napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &syscall_value);
    napi_set_named_property(env, error, "syscall", syscall_value);
  }

  if (path != nullptr && *path != '\0') {
    napi_value path_value = nullptr;
    napi_create_string_utf8(env, path, NAPI_AUTO_LENGTH, &path_value);
    napi_set_named_property(env, error, "path", path_value);
  }

  return error;
}

napi_value CreateNameValue(napi_env env, const uv_dirent_t& ent, const std::string& encoding) {
  if (encoding == "buffer") {
    napi_value out = nullptr;
    const size_t len = std::strlen(ent.name);
    napi_create_buffer_copy(env, len, ent.name, nullptr, &out);
    return out != nullptr ? out : Undefined(env);
  }

  napi_value out = nullptr;
  napi_create_string_utf8(env, ent.name, NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CreateDirentArray(napi_env env, uv_dir_t* dir, int count, const std::string& encoding) {
  napi_value out = nullptr;
  if (count <= 0) {
    napi_get_null(env, &out);
    return out != nullptr ? out : Undefined(env);
  }

  if (napi_create_array_with_length(env, static_cast<size_t>(count) * 2, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }

  for (int i = 0; i < count; ++i) {
    napi_value name = CreateNameValue(env, dir->dirents[i], encoding);
    napi_value type = nullptr;
    napi_create_int32(env, static_cast<int32_t>(dir->dirents[i].type), &type);
    napi_set_element(env, out, static_cast<uint32_t>(i * 2), name);
    napi_set_element(env, out, static_cast<uint32_t>(i * 2 + 1), type != nullptr ? type : Undefined(env));
  }

  return out;
}

DirHandleWrap* UnwrapDirHandle(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<DirHandleWrap*>(data);
}

void CloseDirSync(DirHandleWrap* wrap) {
  if (wrap == nullptr || wrap->dir == nullptr || wrap->closed) return;
  uv_fs_t req;
  int rc = uv_fs_closedir(nullptr, &req, wrap->dir, nullptr);
  uv_fs_req_cleanup(&req);
  if (rc >= 0) {
    wrap->dir = nullptr;
    wrap->closed = true;
    wrap->closing = false;
  }
}

void DirHandleFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<DirHandleWrap*>(data);
  if (wrap == nullptr) return;
  CloseDirSync(wrap);
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

napi_value CreateDirHandle(napi_env env, uv_dir_t* dir) {
  FsDirBindingState* st = GetState(env);
  if (st == nullptr) return Undefined(env);
  napi_value ctor = GetRefValue(env, st->dir_handle_ctor_ref);
  if (ctor == nullptr) return Undefined(env);

  napi_value instance = nullptr;
  if (napi_new_instance(env, ctor, 0, nullptr, &instance) != napi_ok || instance == nullptr) {
    return Undefined(env);
  }

  auto* wrap = new DirHandleWrap();
  wrap->env = env;
  wrap->dir = dir;
  napi_wrap(env, instance, wrap, DirHandleFinalize, nullptr, &wrap->wrapper_ref);
  return instance;
}

void CompleteReq(DirReq* req, napi_value err, napi_value value) {
  if (req == nullptr || req->env == nullptr) return;
  napi_env env = req->env;
  napi_value req_obj = GetRefValue(env, req->req_ref);
  napi_value oncomplete = GetRefValue(env, req->oncomplete_ref);
  if (req_obj == nullptr || oncomplete == nullptr) return;

  if (err != nullptr && !IsUndefined(env, err)) {
    napi_value argv[1] = {err};
    napi_value ignored = nullptr;
    EdgeMakeCallback(env, req_obj, oncomplete, 1, argv, &ignored);
    return;
  }

  if (value != nullptr && !IsUndefined(env, value)) {
    napi_value argv[2] = {Undefined(env), value};
    napi_value ignored = nullptr;
    EdgeMakeCallback(env, req_obj, oncomplete, 2, argv, &ignored);
    return;
  }

  napi_value argv[1] = {Undefined(env)};
  napi_value ignored = nullptr;
  EdgeMakeCallback(env, req_obj, oncomplete, 1, argv, &ignored);
}

void DeleteReq(DirReq* req) {
  if (req == nullptr) return;
  if (req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(req->env, req->active_request_token);
    req->active_request_token = nullptr;
  }
  if (req->handle != nullptr) ReleaseDirHandleRef(req->handle);
  if (!req->req_cleaned) {
    uv_fs_req_cleanup(&req->req);
    req->req_cleaned = true;
  }
  ResetRef(req->env, &req->req_ref);
  ResetRef(req->env, &req->oncomplete_ref);
  delete req;
}

bool InitAsyncReq(napi_env env, napi_value req_value, DirReq* req) {
  if (env == nullptr || req_value == nullptr || req == nullptr) return false;
  napi_value oncomplete = nullptr;
  if (napi_get_named_property(env, req_value, "oncomplete", &oncomplete) != napi_ok || oncomplete == nullptr) {
    return false;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, oncomplete, &type) != napi_ok || type != napi_function) return false;
  req->env = env;
  const bool ok = napi_create_reference(env, req_value, 1, &req->req_ref) == napi_ok &&
                  napi_create_reference(env, oncomplete, 1, &req->oncomplete_ref) == napi_ok;
  if (!ok) return false;
  req->active_request_token =
      EdgeRegisterActiveRequest(env,
                                req_value,
                                ResourceNameForDirReq(req->kind),
                                req,
                                CancelDirReq,
                                GetDirReqOwner);
  return true;
}

void AfterOpenDir(uv_fs_t* uv_req) {
  auto* req = static_cast<DirReq*>(uv_req != nullptr ? uv_req->data : nullptr);
  if (req == nullptr) return;
  if (req->env == nullptr) {
    DeleteReq(req);
    return;
  }
  edge::HandleScope scope(req->env);
  if (!scope.is_open()) {
    DeleteReq(req);
    return;
  }

  napi_value err = nullptr;
  napi_value value = Undefined(req->env);
  if (uv_req->result < 0) {
    err = CreateUvExceptionValue(req->env, static_cast<int>(uv_req->result), "opendir", req->path.c_str());
  } else {
    uv_dir_t* dir = static_cast<uv_dir_t*>(uv_req->ptr);
    value = CreateDirHandle(req->env, dir);
  }

  uv_fs_req_cleanup(&req->req);
  req->req_cleaned = true;
  CompleteReq(req, err, value);
  DeleteReq(req);
}

void AfterReadDir(uv_fs_t* uv_req) {
  auto* req = static_cast<DirReq*>(uv_req != nullptr ? uv_req->data : nullptr);
  if (req == nullptr) return;
  if (req->env == nullptr) {
    DeleteReq(req);
    return;
  }
  edge::HandleScope scope(req->env);
  if (!scope.is_open()) {
    DeleteReq(req);
    return;
  }

  napi_value err = nullptr;
  napi_value value = Undefined(req->env);
  if (uv_req->result < 0) {
    err = CreateUvExceptionValue(req->env, static_cast<int>(uv_req->result), "readdir");
  } else if (uv_req->result == 0) {
    napi_get_null(req->env, &value);
  } else if (req->handle != nullptr && req->handle->dir != nullptr) {
    value = CreateDirentArray(req->env, req->handle->dir, static_cast<int>(uv_req->result), req->encoding);
  }

  uv_fs_req_cleanup(&req->req);
  req->req_cleaned = true;
  CompleteReq(req, err, value);
  DeleteReq(req);
}

void AfterCloseDir(uv_fs_t* uv_req) {
  auto* req = static_cast<DirReq*>(uv_req != nullptr ? uv_req->data : nullptr);
  if (req == nullptr) return;
  if (req->env == nullptr) {
    DeleteReq(req);
    return;
  }
  edge::HandleScope scope(req->env);
  if (!scope.is_open()) {
    DeleteReq(req);
    return;
  }

  napi_value err = nullptr;
  if (uv_req->result < 0) {
    err = CreateUvExceptionValue(req->env, static_cast<int>(uv_req->result), "closedir");
  }

  uv_fs_req_cleanup(&req->req);
  req->req_cleaned = true;
  CompleteReq(req, err, Undefined(req->env));
  DeleteReq(req);
}

napi_value DirHandleCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  return this_arg != nullptr ? this_arg : Undefined(env);
}

napi_value DirHandleRead(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  DirHandleWrap* wrap = UnwrapDirHandle(env, this_arg);
  if (wrap == nullptr || wrap->dir == nullptr || wrap->closed) return Undefined(env);

  const std::string encoding = argc >= 1 ? GetEncoding(env, argv[0]) : "utf8";
  uint32_t buffer_size = 0;
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_uint32(env, argv[1], &buffer_size);
  if (buffer_size != wrap->dirents.size()) {
    wrap->dirents.resize(buffer_size);
    wrap->dir->nentries = static_cast<unsigned int>(wrap->dirents.size());
    wrap->dir->dirents = wrap->dirents.empty() ? nullptr : wrap->dirents.data();
  }

  if (argc >= 3 && argv[2] != nullptr && !IsUndefined(env, argv[2])) {
    auto* req = new DirReq();
    req->kind = DirReqKind::kRead;
    req->handle = wrap;
    req->encoding = encoding;
    if (!InitAsyncReq(env, argv[2], req)) {
      delete req;
      return Undefined(env);
    }
    HoldDirHandleRef(wrap);
    req->req.data = req;
    uv_loop_t* loop = EdgeGetEnvLoop(env);
    const int rc = loop != nullptr ? uv_fs_readdir(loop, &req->req, wrap->dir, AfterReadDir) : UV_EINVAL;
    if (rc < 0) {
      req->req.result = rc;
      AfterReadDir(&req->req);
    }
    return Undefined(env);
  }

  uv_fs_t req;
  const int rc = uv_fs_readdir(nullptr, &req, wrap->dir, nullptr);
  napi_value out = Undefined(env);
  if (rc < 0) {
    uv_fs_req_cleanup(&req);
    napi_throw(env, CreateUvExceptionValue(env, rc, "readdir"));
    return nullptr;
  }

  if (req.result == 0) {
    napi_get_null(env, &out);
  } else {
    out = CreateDirentArray(env, wrap->dir, static_cast<int>(req.result), encoding);
  }
  uv_fs_req_cleanup(&req);
  return out != nullptr ? out : Undefined(env);
}

napi_value DirHandleClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  DirHandleWrap* wrap = UnwrapDirHandle(env, this_arg);
  if (wrap == nullptr) return Undefined(env);

  wrap->closing = false;
  wrap->closed = true;

  if (argc >= 1 && argv[0] != nullptr && !IsUndefined(env, argv[0])) {
    auto* req = new DirReq();
    req->kind = DirReqKind::kClose;
    req->handle = wrap;
    if (!InitAsyncReq(env, argv[0], req)) {
      delete req;
      return Undefined(env);
    }
    HoldDirHandleRef(wrap);
    req->req.data = req;
    uv_loop_t* loop = EdgeGetEnvLoop(env);
    const int rc = loop != nullptr ? uv_fs_closedir(loop, &req->req, wrap->dir, AfterCloseDir) : UV_EINVAL;
    wrap->dir = nullptr;
    if (rc < 0) {
      req->req.result = rc;
      AfterCloseDir(&req->req);
    }
    return Undefined(env);
  }

  if (wrap->dir != nullptr) {
    uv_fs_t req;
    const int rc = uv_fs_closedir(nullptr, &req, wrap->dir, nullptr);
    wrap->dir = nullptr;
    uv_fs_req_cleanup(&req);
    if (rc < 0) {
      napi_throw(env, CreateUvExceptionValue(env, rc, "closedir"));
      return nullptr;
    }
  }
  return Undefined(env);
}

napi_value FsDirOpendir(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  std::string path;
  if (argc < 1 || !ValueToPath(env, argv[0], &path)) return Undefined(env);

  if (argc >= 3 && argv[2] != nullptr && !IsUndefined(env, argv[2])) {
    auto* req = new DirReq();
    req->kind = DirReqKind::kOpen;
    req->encoding = argc >= 2 ? GetEncoding(env, argv[1]) : "utf8";
    req->path = path;
    if (!InitAsyncReq(env, argv[2], req)) {
      delete req;
      return Undefined(env);
    }
    req->req.data = req;
    uv_loop_t* loop = EdgeGetEnvLoop(env);
    const int rc = loop != nullptr ? uv_fs_opendir(loop, &req->req, path.c_str(), AfterOpenDir) : UV_EINVAL;
    if (rc < 0) {
      req->req.result = rc;
      AfterOpenDir(&req->req);
    }
    return Undefined(env);
  }

  uv_fs_t req;
  const int rc = uv_fs_opendir(nullptr, &req, path.c_str(), nullptr);
  if (rc < 0) {
    uv_fs_req_cleanup(&req);
    napi_throw(env, CreateUvExceptionValue(env, rc, "opendir", path.c_str()));
    return nullptr;
  }

  uv_dir_t* dir = static_cast<uv_dir_t*>(req.ptr);
  napi_value out = CreateDirHandle(env, dir);
  uv_fs_req_cleanup(&req);
  return out != nullptr ? out : Undefined(env);
}

napi_value FsDirOpendirSync(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  std::string path;
  if (argc < 1 || !ValueToPath(env, argv[0], &path)) return Undefined(env);

  uv_fs_t req;
  const int rc = uv_fs_opendir(nullptr, &req, path.c_str(), nullptr);
  if (rc < 0) {
    uv_fs_req_cleanup(&req);
    napi_throw(env, CreateUvExceptionValue(env, rc, "opendir", path.c_str()));
    return nullptr;
  }

  uv_dir_t* dir = static_cast<uv_dir_t*>(req.ptr);
  napi_value out = CreateDirHandle(env, dir);
  uv_fs_req_cleanup(&req);
  return out != nullptr ? out : Undefined(env);
}

}  // namespace

napi_value InitFsDir(napi_env env) {
  napi_value binding = EdgeInstallFsDirBinding(env);
  if (binding == nullptr || IsUndefined(env, binding)) return Undefined(env);

  auto& state = EnsureState(env);
  if (state.binding_ref == nullptr) {
    napi_create_reference(env, binding, 1, &state.binding_ref);
  }

  std::vector<napi_property_descriptor> methods = {
      {"read", nullptr, DirHandleRead, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"close", nullptr, DirHandleClose, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  EnsureClassProperty(env, binding, "DirHandle", DirHandleCtor, methods, &state.dir_handle_ctor_ref);
  SetNamedMethod(env, binding, "opendir", FsDirOpendir);
  SetNamedMethod(env, binding, "opendirSync", FsDirOpendirSync);
  return binding;
}

}  // namespace internal_binding
