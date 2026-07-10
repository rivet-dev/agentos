#include "edge_fs.h"
#include "edge_path.h"

#include <cerrno>
#include <cstring>
#include <filesystem>
#include <functional>
#include <fstream>
#include <string>
#include <vector>

#if defined(_WIN32)
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif
#include <sys/stat.h>

#include "node_api.h"
#include "uv.h"

namespace {

void ThrowUVExceptionCopyFile(napi_env env, int errorno, const char* src,
                              const char* dest);

void ThrowInvalidArgType(napi_env env, const char* name, const char* expected) {
  std::string message = "The \"";
  message += (name != nullptr ? name : "value");
  message += "\" argument must be of type ";
  message += (expected != nullptr ? expected : "valid type");
  napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", message.c_str());
}

void ThrowOutOfRangeFd(napi_env env, int32_t value) {
  std::string message =
      "The value of \"fd\" is out of range. It must be >= 0 && <= 2147483647. Received ";
  message += std::to_string(value);
  napi_throw_range_error(env, "ERR_OUT_OF_RANGE", message.c_str());
}

std::string PathFromValue(napi_env env, napi_value value) {
  if (value == nullptr) {
    ThrowInvalidArgType(env, "path", "string or Buffer");
    return "";
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) {
    ThrowInvalidArgType(env, "path", "string or Buffer");
    return "";
  }
  if (type == napi_string) {
    size_t length = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
      ThrowInvalidArgType(env, "path", "string or Buffer");
      return "";
    }
    std::string out(length + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) !=
        napi_ok) {
      ThrowInvalidArgType(env, "path", "string or Buffer");
      return "";
    }
    out.resize(copied);
    return out;
  }

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    size_t length = 0;
    if (napi_get_buffer_info(env, value, &data, &length) != napi_ok || data == nullptr) {
      ThrowInvalidArgType(env, "path", "string or Buffer");
      return "";
    }
    return std::string(static_cast<const char*>(data), length);
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env,
                                 value,
                                 &ta_type,
                                 &length,
                                 &data,
                                 &arraybuffer,
                                 &byte_offset) != napi_ok ||
        data == nullptr) {
      ThrowInvalidArgType(env, "path", "string or Buffer");
      return "";
    }
    if (ta_type != napi_uint8_array && ta_type != napi_uint8_clamped_array) {
      ThrowInvalidArgType(env, "path", "string or Buffer");
      return "";
    }
    return std::string(static_cast<const char*>(data), length);
  }

  ThrowInvalidArgType(env, "path", "string or Buffer");
  return "";
}

bool PathFromValueStrict(napi_env env, napi_value value, std::string* out) {
  if (out == nullptr) return false;
  *out = PathFromValue(env, value);
  bool has_exception = false;
  if (napi_is_exception_pending(env, &has_exception) == napi_ok && has_exception) return false;
  return true;
}

bool ValueToBool(napi_env env, napi_value value, bool* out) {
  if (out == nullptr) return false;
  if (napi_get_value_bool(env, value, out) == napi_ok) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) == napi_ok &&
      (type == napi_undefined || type == napi_null)) {
    *out = false;
    return true;
  }
  return false;
}

bool ValueToDouble(napi_env env, napi_value value, double* out) {
  if (out == nullptr) return false;
  if (napi_get_value_double(env, value, out) == napi_ok) return true;
  return false;
}

static bool IsNullOrUndefined(napi_env env, napi_value value);

std::string PathFromValueUnchecked(napi_env env, napi_value value) {
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
    return "";
  }
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) !=
      napi_ok) {
    return "";
  }
  out.resize(copied);
  return out;
}

void ThrowUVException(napi_env env, int errorno, const char* syscall,
                      const char* path) {
  const char* code = uv_err_name(errorno);
  const char* msg = uv_strerror(errorno);
  if (code == nullptr) code = "UNKNOWN";
  if (msg == nullptr) msg = "unknown error";

  std::string message = std::string(code) + ": " + msg + ", " + syscall;
  if (path != nullptr && path[0] != '\0') {
    message += " '";
    message += path;
    message += "'";
  }

  napi_value msg_val = nullptr;
  if (napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH,
                               &msg_val) != napi_ok ||
      msg_val == nullptr) {
    return;
  }
  napi_value error = nullptr;
  if (napi_create_error(env, nullptr, msg_val, &error) != napi_ok ||
      error == nullptr) {
    return;
  }
  napi_value code_val = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_val) ==
      napi_ok) {
    napi_set_named_property(env, error, "code", code_val);
  }
  napi_value errno_val = nullptr;
  if (napi_create_int32(env, errorno, &errno_val) == napi_ok) {
    napi_set_named_property(env, error, "errno", errno_val);
  }
  napi_value syscall_val = nullptr;
  if (napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &syscall_val) ==
      napi_ok) {
    napi_set_named_property(env, error, "syscall", syscall_val);
  }
  if (path != nullptr && path[0] != '\0') {
    napi_value path_val = nullptr;
    if (napi_create_string_utf8(env, path, NAPI_AUTO_LENGTH, &path_val) ==
        napi_ok) {
      napi_set_named_property(env, error, "path", path_val);
    }
  }
  napi_throw(env, error);
}

void ThrowUVExceptionCopyFile(napi_env env, int errorno, const char* src,
                              const char* dest) {
  const char* code = uv_err_name(errorno);
  const char* msg = uv_strerror(errorno);
  if (code == nullptr) code = "UNKNOWN";
  if (msg == nullptr) msg = "unknown error";
  std::string message = std::string(code) + ": " + msg + ", copyfile ";
  if (src != nullptr && src[0] != '\0') {
    message += "'";
    message += src;
    message += "'";
    if (dest != nullptr && dest[0] != '\0') {
      message += " -> '";
      message += dest;
      message += "'";
    }
  }
  napi_value msg_val = nullptr;
  if (napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH,
                               &msg_val) != napi_ok || msg_val == nullptr) {
    return;
  }
  napi_value error = nullptr;
  if (napi_create_error(env, nullptr, msg_val, &error) != napi_ok ||
      error == nullptr) {
    return;
  }
  napi_value code_val = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_val) ==
      napi_ok) {
    napi_set_named_property(env, error, "code", code_val);
  }
  napi_value errno_val = nullptr;
  if (napi_create_int32(env, errorno, &errno_val) == napi_ok) {
    napi_set_named_property(env, error, "errno", errno_val);
  }
  napi_value syscall_val = nullptr;
  if (napi_create_string_utf8(env, "copyfile", NAPI_AUTO_LENGTH, &syscall_val) ==
      napi_ok) {
    napi_set_named_property(env, error, "syscall", syscall_val);
  }
  if (src != nullptr && src[0] != '\0') {
    napi_value path_val = nullptr;
    if (napi_create_string_utf8(env, src, NAPI_AUTO_LENGTH, &path_val) ==
        napi_ok) {
      napi_set_named_property(env, error, "path", path_val);
    }
  }
  if (dest != nullptr && dest[0] != '\0') {
    napi_value dest_val = nullptr;
    if (napi_create_string_utf8(env, dest, NAPI_AUTO_LENGTH, &dest_val) ==
        napi_ok) {
      napi_set_named_property(env, error, "dest", dest_val);
    }
  }
  napi_throw(env, error);
}

void ThrowErrnoException(napi_env env, int errorno, const char* syscall,
                         const char* message, const char* path) {
  const char* code = uv_err_name(errorno < 0 ? errorno : -errorno);
  if (code == nullptr) code = "ERR_UNKNOWN_ERRNO";
  std::string msg = std::string(code) + ": " + (message != nullptr ? message : strerror(errorno));
  if (syscall != nullptr && syscall[0] != '\0') {
    msg += ", ";
    msg += syscall;
  }
  if (path != nullptr && path[0] != '\0') {
    msg += " '";
    msg += path;
    msg += "'";
  }
  napi_value msg_val = nullptr;
  if (napi_create_string_utf8(env, msg.c_str(), NAPI_AUTO_LENGTH, &msg_val) !=
          napi_ok ||
      msg_val == nullptr) {
    return;
  }
  napi_value error = nullptr;
  if (napi_create_error(env, nullptr, msg_val, &error) != napi_ok ||
      error == nullptr) {
    return;
  }
  napi_value code_val = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_val) ==
      napi_ok) {
    napi_set_named_property(env, error, "code", code_val);
  }
  napi_value errno_val = nullptr;
  if (napi_create_int32(env, errorno, &errno_val) == napi_ok) {
    napi_set_named_property(env, error, "errno", errno_val);
  }
  napi_value syscall_val = nullptr;
  if (napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &syscall_val) ==
      napi_ok) {
    napi_set_named_property(env, error, "syscall", syscall_val);
  }
  if (path != nullptr && path[0] != '\0') {
    napi_value path_val = nullptr;
    if (napi_create_string_utf8(env, path, NAPI_AUTO_LENGTH, &path_val) ==
        napi_ok) {
      napi_set_named_property(env, error, "path", path_val);
    }
  }
  napi_throw(env, error);
}

void SetContextUVError(napi_env env, napi_value ctx, int errorno,
                       const char* syscall) {
  if (ctx == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, ctx, &type) != napi_ok || type != napi_object) return;
  const char* code = uv_err_name(errorno);
  const char* msg = uv_strerror(errorno);
  if (code == nullptr) code = "UNKNOWN";
  if (msg == nullptr) msg = "unknown error";
  napi_value v = nullptr;
  if (napi_create_int32(env, errorno, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, ctx, "errno", v);
  }
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &v) == napi_ok &&
      v != nullptr) {
    napi_set_named_property(env, ctx, "code", v);
  }
  if (napi_create_string_utf8(env, msg, NAPI_AUTO_LENGTH, &v) == napi_ok &&
      v != nullptr) {
    napi_set_named_property(env, ctx, "message", v);
  }
  if (napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &v) == napi_ok &&
      v != nullptr) {
    napi_set_named_property(env, ctx, "syscall", v);
  }
}

napi_value BindingReadFileUtf8(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) {
    return nullptr;
  }
  int32_t flags = 0;
  if (napi_get_value_int32(env, argv[1], &flags) != napi_ok) {
    return nullptr;
  }

  uv_fs_t req;
  uv_file file = uv_fs_open(nullptr, &req, path.c_str(), flags, 0666, nullptr);
  uv_fs_req_cleanup(&req);
  if (file < 0) {
    ThrowUVException(env, static_cast<int>(file), "open", path.c_str());
    return nullptr;
  }

  std::string result;
  char buffer[8192];
  uv_buf_t buf = uv_buf_init(buffer, sizeof(buffer));
  while (true) {
    uv_fs_read(nullptr, &req, file, &buf, 1, -1, nullptr);
    ssize_t r = req.result;
    uv_fs_req_cleanup(&req);
    if (r < 0) {
      uv_fs_close(nullptr, &req, file, nullptr);
      uv_fs_req_cleanup(&req);
      ThrowUVException(env, static_cast<int>(r), "read", nullptr);
      return nullptr;
    }
    if (r <= 0) break;
    result.append(buf.base, static_cast<size_t>(r));
  }

  uv_fs_close(nullptr, &req, file, nullptr);
  uv_fs_req_cleanup(&req);

  napi_value out = nullptr;
  if (napi_create_string_utf8(env, result.c_str(), result.size(), &out) !=
      napi_ok) {
    return nullptr;
  }
  return out;
}

napi_value BindingWriteFileUtf8(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 4) {
    return nullptr;
  }

  bool path_is_fd = false;
  int32_t fd = -1;
  std::string path;
  napi_valuetype path_type = napi_undefined;
  if (napi_typeof(env, argv[0], &path_type) == napi_ok &&
      path_type == napi_number &&
      napi_get_value_int32(env, argv[0], &fd) == napi_ok) {
    path_is_fd = true;
  } else {
    path = PathFromValue(env, argv[0]);
    if (path.empty()) {
      return nullptr;
    }
  }

  size_t data_len = 0;
  if (napi_get_value_string_utf8(env, argv[1], nullptr, 0, &data_len) !=
      napi_ok) {
    return nullptr;
  }
  std::string data(data_len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, argv[1], data.data(), data.size(),
                                 &copied) != napi_ok) {
    return nullptr;
  }
  data.resize(copied);
  int32_t flags = 0;
  int32_t mode = 0;
  if (napi_get_value_int32(env, argv[2], &flags) != napi_ok ||
      napi_get_value_int32(env, argv[3], &mode) != napi_ok) {
    return nullptr;
  }

  uv_fs_t req;
  uv_file file = -1;
  bool close_file = false;
  if (path_is_fd) {
    file = static_cast<uv_file>(fd);
  } else {
    file = uv_fs_open(nullptr, &req, path.c_str(), flags, mode, nullptr);
    uv_fs_req_cleanup(&req);
    if (file < 0) {
      ThrowUVException(env, static_cast<int>(file), "open", path.c_str());
      return nullptr;
    }
    close_file = true;
  }

  size_t offset = 0;
  const size_t length = data.size();
  while (offset < length) {
    uv_buf_t uvbuf =
        uv_buf_init(const_cast<char*>(data.data() + offset),
                    static_cast<unsigned int>(length - offset));
    uv_fs_write(nullptr, &req, file, &uvbuf, 1, -1, nullptr);
    ssize_t bytes_written = req.result;
    uv_fs_req_cleanup(&req);
    if (bytes_written < 0) {
      if (close_file) {
        uv_fs_close(nullptr, &req, file, nullptr);
        uv_fs_req_cleanup(&req);
      }
      ThrowUVException(env, static_cast<int>(bytes_written), "write",
                       path_is_fd ? nullptr : path.c_str());
      return nullptr;
    }
    offset += static_cast<size_t>(bytes_written);
  }

  if (close_file) {
    uv_fs_close(nullptr, &req, file, nullptr);
    uv_fs_req_cleanup(&req);
  }
  return nullptr;
}

const char kPathSeparator = '/';

bool MkdirpSync(std::string path, int mode, std::string* first_path,
                int* out_err) {
  std::vector<std::string> stack;
  stack.push_back(std::move(path));
  uv_fs_t req;

  while (!stack.empty()) {
    std::string next_path = std::move(stack.back());
    stack.pop_back();

    int err = uv_fs_mkdir(nullptr, &req, next_path.c_str(), mode, nullptr);
    uv_fs_req_cleanup(&req);

    while (true) {
      switch (err) {
        case 0:
          if (first_path && first_path->empty()) {
            *first_path = next_path;
          }
          break;
        case UV_EACCES:
        case UV_ENOSPC:
        case UV_ENOTDIR:
        case UV_EPERM:
          *out_err = err;
          return false;
        case UV_ENOENT: {
          size_t sep = next_path.find_last_of(kPathSeparator);
          std::string dirname =
              (sep != std::string::npos) ? next_path.substr(0, sep) : next_path;
          if (dirname != next_path) {
            stack.push_back(next_path);
            stack.push_back(std::move(dirname));
          } else if (stack.empty()) {
            err = UV_EEXIST;
            continue;
          }
          break;
        }
        default: {
          int orig_err = err;
          int stat_err = uv_fs_stat(nullptr, &req, next_path.c_str(), nullptr);
          const uv_stat_t statbuf = req.statbuf;
          uv_fs_req_cleanup(&req);
          if (stat_err == 0 && !S_ISDIR(statbuf.st_mode)) {
            *out_err = (orig_err == UV_EEXIST && !stack.empty()) ? UV_ENOTDIR : UV_EEXIST;
            return false;
          }
          if (stat_err < 0) {
            *out_err = stat_err;
            return false;
          }
          break;
        }
      }
      break;
    }
  }
  *out_err = 0;
  return true;
}

napi_value BindingMkdir(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) {
    return nullptr;
  }
  int32_t mode = 0;
  bool recursive = false;
  if (napi_get_value_int32(env, argv[1], &mode) != napi_ok) {
    return nullptr;
  }
  if (napi_get_value_bool(env, argv[2], &recursive) != napi_ok) {
    return nullptr;
  }

  if (recursive) {
    std::string first_path;
    int err = 0;
    if (!MkdirpSync(path, mode, &first_path, &err)) {
      ThrowUVException(env, err, "mkdir", path.c_str());
      return nullptr;
    }
    if (!first_path.empty()) {
      napi_value ret = nullptr;
      if (napi_create_string_utf8(env, first_path.c_str(), NAPI_AUTO_LENGTH,
                                  &ret) == napi_ok) {
        return ret;
      }
    }
    return nullptr;
  }

  uv_fs_t req;
  int err = uv_fs_mkdir(nullptr, &req, path.c_str(), mode, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "mkdir", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingRmSync(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 4) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) {
    return nullptr;
  }
  int32_t max_retries = 0;
  bool recursive = false;
  int32_t retry_delay = 0;
  if (napi_get_value_int32(env, argv[1], &max_retries) != napi_ok ||
      napi_get_value_bool(env, argv[2], &recursive) != napi_ok ||
      napi_get_value_int32(env, argv[3], &retry_delay) != napi_ok) {
    return nullptr;
  }

  namespace fs = std::filesystem;
  std::error_code error;
  auto file_status = fs::symlink_status(path, error);
  if (file_status.type() == fs::file_type::not_found) {
    return nullptr;
  }
  if (file_status.type() == fs::file_type::directory && !recursive) {
    ThrowErrnoException(env, EISDIR, "rm", "Path is a directory", path.c_str());
    return nullptr;
  }

  auto can_retry = [](const std::error_code& ec) {
    return ec == std::errc::device_or_resource_busy ||
           ec == std::errc::too_many_files_open ||
           ec == std::errc::too_many_files_open_in_system ||
           ec == std::errc::directory_not_empty ||
           ec == std::errc::operation_not_permitted;
  };

  std::function<void(const fs::path&, std::error_code&)> remove_entry =
      [&](const fs::path& entry_path, std::error_code& ec) {
        if (ec) return;
        const fs::file_status status = fs::symlink_status(entry_path, ec);
        if (ec || status.type() == fs::file_type::not_found) return;

        if (status.type() == fs::file_type::directory) {
          fs::remove(entry_path, ec);
          if (!ec || ec == std::errc::no_such_file_or_directory) {
            ec.clear();
            return;
          }
          if (ec != std::errc::directory_not_empty) {
            return;
          }
          ec.clear();

          for (const auto& child : fs::directory_iterator(entry_path, ec)) {
            if (ec) return;
            remove_entry(child.path(), ec);
            if (ec) return;
          }
        }

        fs::remove(entry_path, ec);
      };

  int i = 1;
  while (max_retries >= 0) {
    error.clear();
    if (recursive) {
      remove_entry(fs::path(path), error);
    } else {
      fs::remove(path, error);
    }
    if (!error || error == std::errc::no_such_file_or_directory) {
      return nullptr;
    }
    if (!can_retry(error)) {
      break;
    }
    if (retry_delay > 0) {
#ifdef _WIN32
      Sleep(static_cast<DWORD>(i * retry_delay / 1000));
#else
      sleep(static_cast<unsigned>(i * retry_delay / 1000));
#endif
    }
    max_retries--;
    i++;
  }

  int errno_val = error.value();
#ifdef _WIN32
  int permission_denied_errno = EPERM;
#else
  int permission_denied_errno = EACCES;
#endif
  if (error == std::errc::operation_not_permitted) {
    ThrowErrnoException(env, EPERM, "rm", "Operation not permitted",
                        path.c_str());
  } else if (error == std::errc::directory_not_empty) {
    ThrowErrnoException(env, ENOTEMPTY, "rm", "Directory not empty",
                        path.c_str());
  } else if (error == std::errc::not_a_directory) {
    ThrowErrnoException(env, ENOTDIR, "rm", "Not a directory", path.c_str());
  } else if (error == std::errc::permission_denied) {
    ThrowErrnoException(env, permission_denied_errno, "rm", "Permission denied",
                        path.c_str());
  } else {
    ThrowErrnoException(env, UV_UNKNOWN, "rm",
                       ("Unknown error: " + error.message()).c_str(),
                       path.c_str());
  }
  return nullptr;
}

napi_value BindingReaddir(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) {
    return nullptr;
  }
  bool with_file_types = false;
  if (napi_get_value_bool(env, argv[1], &with_file_types) != napi_ok) {
    return nullptr;
  }

  uv_fs_t req;
  int err = uv_fs_scandir(nullptr, &req, path.c_str(), 0, nullptr);
  if (err < 0) {
    uv_fs_req_cleanup(&req);
    ThrowUVException(env, err, "scandir", path.c_str());
    return nullptr;
  }

  std::vector<std::string> names;
  std::vector<int> types;
  uv_dirent_t ent;
  while (true) {
    int r = uv_fs_scandir_next(&req, &ent);
    if (r == UV_EOF) break;
    if (r < 0) {
      uv_fs_req_cleanup(&req);
      ThrowUVException(env, r, "scandir", path.c_str());
      return nullptr;
    }
    names.emplace_back(ent.name);
    if (with_file_types) {
      types.push_back(static_cast<int>(ent.type));
    }
  }
  uv_fs_req_cleanup(&req);

  napi_value names_array = nullptr;
  if (napi_create_array_with_length(env, names.size(), &names_array) !=
      napi_ok) {
    return nullptr;
  }
  for (size_t i = 0; i < names.size(); i++) {
    napi_value name_val = nullptr;
    if (napi_create_string_utf8(env, names[i].c_str(), NAPI_AUTO_LENGTH,
                                &name_val) == napi_ok) {
      napi_set_element(env, names_array, static_cast<uint32_t>(i), name_val);
    }
  }

  if (!with_file_types) {
    return names_array;
  }

  napi_value types_array = nullptr;
  if (napi_create_array_with_length(env, types.size(), &types_array) !=
      napi_ok) {
    return names_array;
  }
  for (size_t i = 0; i < types.size(); i++) {
    napi_value type_val = nullptr;
    if (napi_create_int32(env, types[i], &type_val) == napi_ok) {
      napi_set_element(env, types_array, static_cast<uint32_t>(i), type_val);
    }
  }

  napi_value result = nullptr;
  if (napi_create_array_with_length(env, 2, &result) != napi_ok) {
    return names_array;
  }
  napi_set_element(env, result, 0, names_array);
  napi_set_element(env, result, 1, types_array);
  return result;
}

napi_value BindingRealpath(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = edge_path::ToNamespacedPath(PathFromValue(env, argv[0]));
  if (path.empty()) {
    return nullptr;
  }

  uv_fs_t req;
  int err = uv_fs_realpath(nullptr, &req, path.c_str(), nullptr);
  if (err < 0) {
    uv_fs_req_cleanup(&req);
    ThrowUVException(env, err, "realpath", path.c_str());
    return nullptr;
  }
  const std::string resolved =
      edge_path::FromNamespacedPath(static_cast<const char*>(req.ptr));
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, resolved.c_str(), NAPI_AUTO_LENGTH, &out) !=
      napi_ok) {
    uv_fs_req_cleanup(&req);
    return nullptr;
  }
  uv_fs_req_cleanup(&req);
  return out;
}

// Node FsStatsOffset order: dev, mode, nlink, uid, gid, rdev, blksize, ino, size,
// blocks, atime_sec, atime_nsec, mtime_sec, mtime_nsec, ctime_sec, ctime_nsec,
// birthtime_sec, birthtime_nsec (18 elements).
constexpr size_t kStatArrayLength = 18;

void StatToArray(const uv_stat_t* s, double* out) {
  out[0] = static_cast<double>(s->st_dev);
  out[1] = static_cast<double>(s->st_mode);
  out[2] = static_cast<double>(s->st_nlink);
  out[3] = static_cast<double>(s->st_uid);
  out[4] = static_cast<double>(s->st_gid);
  out[5] = static_cast<double>(s->st_rdev);
  out[6] = static_cast<double>(s->st_blksize);
  out[7] = static_cast<double>(s->st_ino);
  out[8] = static_cast<double>(s->st_size);
  out[9] = static_cast<double>(s->st_blocks);
  out[10] = static_cast<double>(s->st_atim.tv_sec);
  out[11] = static_cast<double>(s->st_atim.tv_nsec);
  out[12] = static_cast<double>(s->st_mtim.tv_sec);
  out[13] = static_cast<double>(s->st_mtim.tv_nsec);
  out[14] = static_cast<double>(s->st_ctim.tv_sec);
  out[15] = static_cast<double>(s->st_ctim.tv_nsec);
  out[16] = static_cast<double>(s->st_birthtim.tv_sec);
  out[17] = static_cast<double>(s->st_birthtim.tv_nsec);
}

napi_value StatArrayToNapi(napi_env env, const double* arr) {
  napi_value result = nullptr;
  if (napi_create_array_with_length(env, kStatArrayLength, &result) !=
      napi_ok ||
      result == nullptr) {
    return nullptr;
  }
  for (size_t i = 0; i < kStatArrayLength; i++) {
    napi_value v = nullptr;
    if (napi_create_double(env, arr[i], &v) != napi_ok) continue;
    if (napi_set_element(env, result, static_cast<uint32_t>(i), v) != napi_ok)
      continue;
  }
  return result;
}

napi_value BindingStat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;

  uv_fs_t req;
  int err = uv_fs_stat(nullptr, &req, path.c_str(), nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "stat", path.c_str());
    return nullptr;
  }
  double arr[kStatArrayLength];
  StatToArray(&req.statbuf, arr);
  return StatArrayToNapi(env, arr);
}

napi_value BindingLstat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;

  uv_fs_t req;
  int err = uv_fs_lstat(nullptr, &req, path.c_str(), nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "lstat", path.c_str());
    return nullptr;
  }
  double arr[kStatArrayLength];
  StatToArray(&req.statbuf, arr);
  return StatArrayToNapi(env, arr);
}

napi_value BindingFstat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) {
    ThrowInvalidArgType(env, "fd", "number");
    return nullptr;
  }
  if (fd < 0) {
    ThrowOutOfRangeFd(env, fd);
    return nullptr;
  }

  uv_fs_t req;
  int err = uv_fs_fstat(nullptr, &req, fd, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "fstat", nullptr);
    return nullptr;
  }
  double arr[kStatArrayLength];
  StatToArray(&req.statbuf, arr);
  return StatArrayToNapi(env, arr);
}

napi_value BindingExistsSync(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) {
    napi_value false_val = nullptr;
    if (napi_get_boolean(env, false, &false_val) == napi_ok) return false_val;
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_access(nullptr, &req, path.c_str(), 0, nullptr);
  uv_fs_req_cleanup(&req);
  napi_value result = nullptr;
  if (napi_get_boolean(env, err == 0, &result) != napi_ok) return nullptr;
  return result;
}

napi_value BindingAccessSync(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  int32_t mode = F_OK;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefined(env, argv[1]) &&
      napi_get_value_int32(env, argv[1], &mode) != napi_ok) {
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_access(nullptr, &req, path.c_str(), mode, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "access", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingOpen(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  int32_t flags = UV_FS_O_RDONLY;
  int32_t mode = 0666;
  if (argc >= 2 && napi_get_value_int32(env, argv[1], &flags) != napi_ok) {
    return nullptr;
  }
  if (argc >= 3 && napi_get_value_int32(env, argv[2], &mode) != napi_ok) {
    return nullptr;
  }
  uv_fs_t req;
  uv_file file = uv_fs_open(nullptr, &req, path.c_str(), flags, mode, nullptr);
  uv_fs_req_cleanup(&req);
  if (file < 0) {
    ThrowUVException(env, static_cast<int>(file), "open", path.c_str());
    return nullptr;
  }
  napi_value out = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(file), &out) != napi_ok) {
    return nullptr;
  }
  return out;
}

napi_value BindingClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  uv_fs_t req;
  int err = uv_fs_close(nullptr, &req, fd, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "close", nullptr);
    return nullptr;
  }
  return nullptr;
}

// Get pointer and byte length from a Buffer or TypedArray (e.g. Uint8Array).
// Returns true on success; *data and *byte_length are set.
static bool BufferFromValue(napi_env env, napi_value value, void** data,
                            size_t* byte_length) {
  if (value == nullptr || data == nullptr || byte_length == nullptr) {
    return false;
  }
  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) != napi_ok) return false;
  if (is_buffer) {
    size_t len = 0;
    if (napi_get_buffer_info(env, value, data, &len) != napi_ok) return false;
    *byte_length = len;
    return true;
  }
  bool is_ta = false;
  if (napi_is_typedarray(env, value, &is_ta) != napi_ok || !is_ta) {
    return false;
  }
  napi_typedarray_type type = napi_uint8_array;
  size_t element_length = 0;
  void* ptr = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(env, value, &type, &element_length, &ptr,
                               nullptr, &byte_offset) != napi_ok) {
    return false;
  }
  static const size_t kBytesPerElement[] = {
      1, 1, 2, 2, 4, 4, 4, 8, 8, 8, 8, 8, 8, 2, 2, 4, 4, 8, 8,
  };
  const size_t kNumTypes = sizeof(kBytesPerElement) / sizeof(kBytesPerElement[0]);
  const size_t el_size = (static_cast<size_t>(type) < kNumTypes)
                             ? kBytesPerElement[static_cast<size_t>(type)]
                             : 1;
  *data = ptr;
  *byte_length = element_length * el_size;
  return true;
}

static bool IsNullOrUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  return type == napi_null || type == napi_undefined;
}

napi_value BindingReadSync(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  void* buf_data = nullptr;
  size_t buf_byte_length = 0;
  if (!BufferFromValue(env, argv[1], &buf_data, &buf_byte_length)) {
    return nullptr;
  }
  size_t offset = 0;
  size_t length = buf_byte_length;
  int64_t position = -1;
  if (argc >= 3 && argv[2] != nullptr) {
    if (napi_get_value_uint32(env, argv[2], reinterpret_cast<uint32_t*>(&offset)) != napi_ok) {
      return nullptr;
    }
  }
  if (argc >= 4 && argv[3] != nullptr) {
    if (napi_get_value_uint32(env, argv[3], reinterpret_cast<uint32_t*>(&length)) != napi_ok) {
      return nullptr;
    }
  }
  if (argc >= 5 && argv[4] != nullptr) {
    if (!IsNullOrUndefined(env, argv[4])) {
      if (napi_get_value_int64(env, argv[4], &position) != napi_ok) return nullptr;
    }
  }
  if (offset > buf_byte_length || length > buf_byte_length - offset) {
    return nullptr;
  }
  if (length == 0) {
    napi_value zero = nullptr;
    if (napi_create_int32(env, 0, &zero) != napi_ok) return nullptr;
    return zero;
  }
  uv_buf_t uv_buf =
      uv_buf_init(static_cast<char*>(buf_data) + offset, static_cast<unsigned int>(length));
  uv_fs_t req;
  int r = uv_fs_read(nullptr, &req, fd, &uv_buf, 1, position, nullptr);
  uv_fs_req_cleanup(&req);
  if (r < 0) {
    ThrowUVException(env, r, "read", nullptr);
    return nullptr;
  }
  napi_value result = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(r), &result) != napi_ok) {
    return nullptr;
  }
  return result;
}

napi_value BindingWriteSync(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  void* buf_data = nullptr;
  size_t buf_byte_length = 0;
  if (!BufferFromValue(env, argv[1], &buf_data, &buf_byte_length)) {
    return nullptr;
  }
  int64_t offset_i = 0;
  int64_t length_i = static_cast<int64_t>(buf_byte_length);
  int64_t position = -1;
  if (argc >= 3 && argv[2] != nullptr) {
    if (napi_get_value_int64(env, argv[2], &offset_i) != napi_ok) return nullptr;
  }
  if (argc >= 4 && argv[3] != nullptr) {
    if (napi_get_value_int64(env, argv[3], &length_i) != napi_ok) return nullptr;
  }
  if (argc >= 5 && argv[4] != nullptr) {
    if (!IsNullOrUndefined(env, argv[4]) &&
        napi_get_value_int64(env, argv[4], &position) != napi_ok) {
      return nullptr;
    }
  }
  if (offset_i < 0 || length_i < 0 ||
      static_cast<size_t>(offset_i) > buf_byte_length ||
      static_cast<size_t>(length_i) > buf_byte_length - static_cast<size_t>(offset_i)) {
    return nullptr;
  }
  size_t offset = static_cast<size_t>(offset_i);
  size_t length = static_cast<size_t>(length_i);
  if (length == 0) {
    napi_value zero = nullptr;
    if (napi_create_int32(env, 0, &zero) != napi_ok) return nullptr;
    return zero;
  }
  uv_buf_t uv_buf = uv_buf_init(static_cast<char*>(buf_data) + offset,
                                 static_cast<unsigned int>(length));
  uv_fs_t req;
  int r = uv_fs_write(nullptr, &req, fd, &uv_buf, 1, position, nullptr);
  uv_fs_req_cleanup(&req);
  if (r < 0) {
    ThrowUVException(env, r, "write", nullptr);
    return nullptr;
  }
  napi_value result = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(r), &result) != napi_ok) {
    return nullptr;
  }
  return result;
}

napi_value BindingWriteSyncString(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  size_t data_len = 0;
  if (napi_get_value_string_utf8(env, argv[1], nullptr, 0, &data_len) != napi_ok) {
    return nullptr;
  }
  std::string data(data_len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, argv[1], data.data(), data.size(), &copied) != napi_ok) {
    return nullptr;
  }
  data.resize(copied);
  if (data.empty()) {
    napi_value zero = nullptr;
    if (napi_create_int32(env, 0, &zero) != napi_ok) return nullptr;
    return zero;
  }
  uv_buf_t uv_buf = uv_buf_init(const_cast<char*>(data.data()),
                                static_cast<unsigned int>(data.size()));
  uv_fs_t req;
  int r = uv_fs_write(nullptr, &req, fd, &uv_buf, 1, -1, nullptr);
  uv_fs_req_cleanup(&req);
  if (r < 0) {
    ThrowUVException(env, r, "write", nullptr);
    return nullptr;
  }
  napi_value result = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(r), &result) != napi_ok) {
    return nullptr;
  }
  return result;
}

napi_value BindingWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  void* buf_data = nullptr;
  size_t buf_byte_length = 0;
  if (!BufferFromValue(env, argv[1], &buf_data, &buf_byte_length)) {
    return nullptr;
  }
  int64_t offset_i = 0;
  int64_t length_i = static_cast<int64_t>(buf_byte_length);
  int64_t position = -1;
  if (argc >= 3 && argv[2] != nullptr &&
      napi_get_value_int64(env, argv[2], &offset_i) != napi_ok) {
    return nullptr;
  }
  if (argc >= 4 && argv[3] != nullptr &&
      napi_get_value_int64(env, argv[3], &length_i) != napi_ok) {
    return nullptr;
  }
  if (argc >= 5 && argv[4] != nullptr) {
    if (!IsNullOrUndefined(env, argv[4]) &&
        napi_get_value_int64(env, argv[4], &position) != napi_ok) {
      return nullptr;
    }
  }
  napi_value ctx = (argc >= 7) ? argv[6] : nullptr;
  if (offset_i < 0 || length_i < 0 ||
      static_cast<size_t>(offset_i) > buf_byte_length ||
      static_cast<size_t>(length_i) >
          buf_byte_length - static_cast<size_t>(offset_i)) {
    SetContextUVError(env, ctx, UV_EINVAL, "write");
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  size_t offset = static_cast<size_t>(offset_i);
  size_t length = static_cast<size_t>(length_i);
  uv_buf_t uv_buf = uv_buf_init(static_cast<char*>(buf_data) + offset,
                                static_cast<unsigned int>(length));
  uv_fs_t req;
  int r = uv_fs_write(nullptr, &req, fd, &uv_buf, 1, position, nullptr);
  uv_fs_req_cleanup(&req);
  if (r < 0) {
    SetContextUVError(env, ctx, r, "write");
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value result = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(r), &result) != napi_ok) {
    return nullptr;
  }
  return result;
}

napi_value BindingRename(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string old_path = PathFromValue(env, argv[0]);
  std::string new_path = PathFromValue(env, argv[1]);
  if (old_path.empty() || new_path.empty()) return nullptr;
  uv_fs_t req;
  int err = uv_fs_rename(nullptr, &req, old_path.c_str(), new_path.c_str(), nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "rename", old_path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingUnlink(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  uv_fs_t req;
  int err = uv_fs_unlink(nullptr, &req, path.c_str(), nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "unlink", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingRmdir(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  uv_fs_t req;
  int err = uv_fs_rmdir(nullptr, &req, path.c_str(), nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "rmdir", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingFtruncate(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  int64_t len = 0;
  if (napi_get_value_int64(env, argv[1], &len) != napi_ok) return nullptr;
  uv_fs_t req;
  int err = uv_fs_ftruncate(nullptr, &req, fd, len, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "ftruncate", nullptr);
    return nullptr;
  }
  return nullptr;
}

napi_value BindingCopyFile(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string src = PathFromValue(env, argv[0]);
  std::string dest = PathFromValue(env, argv[1]);
  if (src.empty() || dest.empty()) return nullptr;
  int32_t flags = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    if (napi_get_value_int32(env, argv[2], &flags) != napi_ok) return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_copyfile(nullptr, &req, src.c_str(), dest.c_str(), flags, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVExceptionCopyFile(env, err, src.c_str(), dest.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingReadlink(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  uv_fs_t req;
  int err = uv_fs_readlink(nullptr, &req, path.c_str(), nullptr);
  if (err < 0) {
    uv_fs_req_cleanup(&req);
    ThrowUVException(env, err, "readlink", path.c_str());
    return nullptr;
  }
  const char* link_path = static_cast<const char*>(req.ptr);
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, link_path ? link_path : "", NAPI_AUTO_LENGTH, &out) != napi_ok) {
    uv_fs_req_cleanup(&req);
    return nullptr;
  }
  uv_fs_req_cleanup(&req);
  return out;
}

napi_value BindingSymlink(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string target = PathFromValue(env, argv[0]);
  std::string path = PathFromValue(env, argv[1]);
  if (target.empty() || path.empty()) return nullptr;
  int32_t flags = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    if (napi_get_value_int32(env, argv[2], &flags) != napi_ok) return nullptr;
  }
#if defined(_WIN32)
  uv_fs_t req;
  int err = uv_fs_symlink(nullptr, &req, target.c_str(), path.c_str(), flags, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "symlink", path.c_str());
    return nullptr;
  }
#else
  (void)flags;
  if (::symlink(target.c_str(), path.c_str()) != 0) {
    ThrowErrnoException(env, errno, "symlink", strerror(errno), path.c_str());
    return nullptr;
  }
#endif
  return nullptr;
}

napi_value BindingChmod(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  int32_t mode = 0;
  if (napi_get_value_int32(env, argv[1], &mode) != napi_ok) return nullptr;
  uv_fs_t req;
  int err = uv_fs_chmod(nullptr, &req, path.c_str(), mode, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "chmod", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingFchmod(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  int32_t mode = 0;
  if (napi_get_value_int32(env, argv[1], &mode) != napi_ok) return nullptr;
  uv_fs_t req;
  int err = uv_fs_fchmod(nullptr, &req, fd, mode, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "fchmod", nullptr);
    return nullptr;
  }
  return nullptr;
}

napi_value BindingChown(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  uint32_t uid = 0;
  uint32_t gid = 0;
  if (napi_get_value_uint32(env, argv[1], &uid) != napi_ok ||
      napi_get_value_uint32(env, argv[2], &gid) != napi_ok) {
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_chown(nullptr, &req, path.c_str(), uid, gid, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "chown", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingFchown(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  int32_t fd = 0;
  uint32_t uid = 0;
  uint32_t gid = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok ||
      napi_get_value_uint32(env, argv[1], &uid) != napi_ok ||
      napi_get_value_uint32(env, argv[2], &gid) != napi_ok) {
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_fchown(nullptr, &req, fd, uid, gid, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "fchown", nullptr);
    return nullptr;
  }
  return nullptr;
}

napi_value BindingLchown(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  uint32_t uid = 0;
  uint32_t gid = 0;
  if (napi_get_value_uint32(env, argv[1], &uid) != napi_ok ||
      napi_get_value_uint32(env, argv[2], &gid) != napi_ok) {
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_lchown(nullptr, &req, path.c_str(), uid, gid, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "lchown", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingUtimes(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  double atime = 0, mtime = 0;
  if (!ValueToDouble(env, argv[1], &atime)) {
    ThrowInvalidArgType(env, "atime", "number");
    return nullptr;
  }
  if (!ValueToDouble(env, argv[2], &mtime)) {
    ThrowInvalidArgType(env, "mtime", "number");
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_utime(nullptr, &req, path.c_str(), atime, mtime, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "utime", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingFutimes(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) {
    ThrowInvalidArgType(env, "fd", "number");
    return nullptr;
  }
  if (fd < 0) {
    ThrowOutOfRangeFd(env, fd);
    return nullptr;
  }
  double atime = 0, mtime = 0;
  if (!ValueToDouble(env, argv[1], &atime)) {
    ThrowInvalidArgType(env, "atime", "number");
    return nullptr;
  }
  if (!ValueToDouble(env, argv[2], &mtime)) {
    ThrowInvalidArgType(env, "mtime", "number");
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_futime(nullptr, &req, fd, atime, mtime, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "futime", nullptr);
    return nullptr;
  }
  return nullptr;
}

napi_value BindingLutimes(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 3) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  double atime = 0, mtime = 0;
  if (!ValueToDouble(env, argv[1], &atime)) {
    ThrowInvalidArgType(env, "atime", "number");
    return nullptr;
  }
  if (!ValueToDouble(env, argv[2], &mtime)) {
    ThrowInvalidArgType(env, "mtime", "number");
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_lutime(nullptr, &req, path.c_str(), atime, mtime, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "lutime", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingFsync(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  uv_fs_t req;
  int err = uv_fs_fsync(nullptr, &req, fd, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "fsync", nullptr);
    return nullptr;
  }
  return nullptr;
}

napi_value BindingFdatasync(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  int32_t fd = 0;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok) return nullptr;
  uv_fs_t req;
  int err = uv_fs_fdatasync(nullptr, &req, fd, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "fdatasync", nullptr);
    return nullptr;
  }
  return nullptr;
}

napi_value BindingMkdtemp(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 1) {
    return nullptr;
  }
  std::string tmpl = PathFromValue(env, argv[0]);
  if (tmpl.empty()) return nullptr;
  // Match Node: append exactly 6 X's (no stripping). Template = prefix + "XXXXXX".
  static constexpr const char suffix[] = "XXXXXX";
  tmpl.append(suffix);
  uv_fs_t req;
  int err = uv_fs_mkdtemp(nullptr, &req, tmpl.c_str(), nullptr);
  if (err < 0) {
    uv_fs_req_cleanup(&req);
    ThrowUVException(env, err, "mkdtemp", tmpl.c_str());
    return nullptr;
  }
  const char* path = req.path;
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, path ? path : tmpl.c_str(), NAPI_AUTO_LENGTH, &out) != napi_ok) {
    uv_fs_req_cleanup(&req);
    return nullptr;
  }
  uv_fs_req_cleanup(&req);
  return out;
}

napi_value BindingAccess(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string path = PathFromValue(env, argv[0]);
  if (path.empty()) return nullptr;
  int32_t mode = F_OK;
  if (argv[1] != nullptr && !IsNullOrUndefined(env, argv[1]) &&
      napi_get_value_int32(env, argv[1], &mode) != napi_ok) {
    return nullptr;
  }
  uv_fs_t req;
  int err = uv_fs_access(nullptr, &req, path.c_str(), mode, nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "access", path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingLink(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 2) {
    return nullptr;
  }
  std::string existing_path = PathFromValue(env, argv[0]);
  std::string new_path = PathFromValue(env, argv[1]);
  if (existing_path.empty() || new_path.empty()) return nullptr;
  uv_fs_t req;
  int err = uv_fs_link(nullptr, &req, existing_path.c_str(), new_path.c_str(), nullptr);
  uv_fs_req_cleanup(&req);
  if (err < 0) {
    ThrowUVException(env, err, "link", new_path.c_str());
    return nullptr;
  }
  return nullptr;
}

napi_value BindingGetFormatOfExtensionlessFile(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) {
    return nullptr;
  }

  const std::string path = PathFromValue(env, argv[0]);
  bool is_wasm = false;
  if (!path.empty()) {
    std::ifstream in(path, std::ios::binary);
    if (in.good()) {
      char header[4] = {0, 0, 0, 0};
      in.read(header, sizeof(header));
      is_wasm = in.gcount() == static_cast<std::streamsize>(sizeof(header)) &&
                header[0] == '\0' &&
                header[1] == 'a' &&
                header[2] == 's' &&
                header[3] == 'm';
    }
  }

  napi_value out = nullptr;
  if (napi_create_int32(env, is_wasm ? 1 : 0, &out) != napi_ok) return nullptr;
  return out;
}

void SetMethod(napi_env env, napi_value obj, const char* name,
               napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) !=
      napi_ok) {
    return;
  }
  napi_set_named_property(env, obj, name, fn);
}

void SetInt32Constant(napi_env env, napi_value obj, const char* name,
                      int32_t value) {
  napi_value val = nullptr;
  if (napi_create_int32(env, value, &val) != napi_ok) return;
  napi_set_named_property(env, obj, name, val);
}

}  // namespace

napi_value EdgeInstallFsBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) {
    return nullptr;
  }

  SetMethod(env, binding, "readFileUtf8", BindingReadFileUtf8);
  SetMethod(env, binding, "writeFileUtf8", BindingWriteFileUtf8);
  SetMethod(env, binding, "mkdir", BindingMkdir);
  SetMethod(env, binding, "rmSync", BindingRmSync);
  SetMethod(env, binding, "readdir", BindingReaddir);
  SetMethod(env, binding, "realpath", BindingRealpath);
  SetMethod(env, binding, "existsSync", BindingExistsSync);
  SetMethod(env, binding, "access", BindingAccess);
  SetMethod(env, binding, "accessSync", BindingAccessSync);
  SetMethod(env, binding, "stat", BindingStat);
  SetMethod(env, binding, "lstat", BindingLstat);
  SetMethod(env, binding, "fstat", BindingFstat);
  SetMethod(env, binding, "open", BindingOpen);
  SetMethod(env, binding, "close", BindingClose);
  SetMethod(env, binding, "readSync", BindingReadSync);
  SetMethod(env, binding, "writeSync", BindingWriteSync);
  SetMethod(env, binding, "writeSyncString", BindingWriteSyncString);
  SetMethod(env, binding, "writeBuffer", BindingWriteBuffer);
  SetMethod(env, binding, "rename", BindingRename);
  SetMethod(env, binding, "unlink", BindingUnlink);
  SetMethod(env, binding, "rmdir", BindingRmdir);
  SetMethod(env, binding, "ftruncate", BindingFtruncate);
  SetMethod(env, binding, "copyFile", BindingCopyFile);
  SetMethod(env, binding, "readlink", BindingReadlink);
  SetMethod(env, binding, "symlink", BindingSymlink);
  SetMethod(env, binding, "chmod", BindingChmod);
  SetMethod(env, binding, "fchmod", BindingFchmod);
  SetMethod(env, binding, "chown", BindingChown);
  SetMethod(env, binding, "fchown", BindingFchown);
  SetMethod(env, binding, "lchown", BindingLchown);
  SetMethod(env, binding, "utimes", BindingUtimes);
  SetMethod(env, binding, "futimes", BindingFutimes);
  SetMethod(env, binding, "lutimes", BindingLutimes);
  SetMethod(env, binding, "fsync", BindingFsync);
  SetMethod(env, binding, "fdatasync", BindingFdatasync);
  SetMethod(env, binding, "mkdtemp", BindingMkdtemp);
  SetMethod(env, binding, "link", BindingLink);
  SetMethod(env, binding, "getFormatOfExtensionlessFile", BindingGetFormatOfExtensionlessFile);

  SetInt32Constant(env, binding, "O_RDONLY", UV_FS_O_RDONLY);
  SetInt32Constant(env, binding, "O_WRONLY", UV_FS_O_WRONLY);
  SetInt32Constant(env, binding, "O_RDWR", UV_FS_O_RDWR);
  SetInt32Constant(env, binding, "O_CREAT", UV_FS_O_CREAT);
  SetInt32Constant(env, binding, "O_TRUNC", UV_FS_O_TRUNC);
  SetInt32Constant(env, binding, "O_APPEND", UV_FS_O_APPEND);
  SetInt32Constant(env, binding, "O_EXCL", UV_FS_O_EXCL);
  SetInt32Constant(env, binding, "O_SYNC", UV_FS_O_SYNC);
#if defined(O_NOATIME)
  SetInt32Constant(env, binding, "O_NOATIME", static_cast<int32_t>(O_NOATIME));
#endif

  SetInt32Constant(env, binding, "UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN);
  SetInt32Constant(env, binding, "UV_DIRENT_FILE", UV_DIRENT_FILE);
  SetInt32Constant(env, binding, "UV_DIRENT_DIR", UV_DIRENT_DIR);
  SetInt32Constant(env, binding, "UV_DIRENT_LINK", UV_DIRENT_LINK);
  SetInt32Constant(env, binding, "UV_DIRENT_FIFO", UV_DIRENT_FIFO);
  SetInt32Constant(env, binding, "UV_DIRENT_SOCKET", UV_DIRENT_SOCKET);
  SetInt32Constant(env, binding, "UV_DIRENT_CHAR", UV_DIRENT_CHAR);
  SetInt32Constant(env, binding, "UV_DIRENT_BLOCK", UV_DIRENT_BLOCK);

#if defined(_WIN32)
  SetInt32Constant(env, binding, "F_OK", 0);
  SetInt32Constant(env, binding, "R_OK", 4);
  SetInt32Constant(env, binding, "W_OK", 2);
  SetInt32Constant(env, binding, "X_OK", 1);
#else
  SetInt32Constant(env, binding, "F_OK", F_OK);
  SetInt32Constant(env, binding, "R_OK", R_OK);
  SetInt32Constant(env, binding, "W_OK", W_OK);
  SetInt32Constant(env, binding, "X_OK", X_OK);
#endif
  // File type constants (portable; match Node's Stats.is* checks)
  SetInt32Constant(env, binding, "S_IFMT", 0170000);
  SetInt32Constant(env, binding, "S_IFREG", 0100000);
  SetInt32Constant(env, binding, "S_IFDIR", 0040000);
  SetInt32Constant(env, binding, "S_IFBLK", 0060000);
  SetInt32Constant(env, binding, "S_IFCHR", 0020000);
  SetInt32Constant(env, binding, "S_IFLNK", 0120000);
  SetInt32Constant(env, binding, "S_IFIFO", 0010000);
  SetInt32Constant(env, binding, "S_IFSOCK", 0140000);
  SetInt32Constant(env, binding, "S_IRWXU", 0700);
  SetInt32Constant(env, binding, "S_IRUSR", 0400);
  SetInt32Constant(env, binding, "S_IWUSR", 0200);
  SetInt32Constant(env, binding, "S_IXUSR", 0100);
  SetInt32Constant(env, binding, "S_IRWXG", 0070);
  SetInt32Constant(env, binding, "S_IRGRP", 0040);
  SetInt32Constant(env, binding, "S_IWGRP", 0020);
  SetInt32Constant(env, binding, "S_IXGRP", 0010);
  SetInt32Constant(env, binding, "S_IRWXO", 0007);
  SetInt32Constant(env, binding, "S_IROTH", 0004);
  SetInt32Constant(env, binding, "S_IWOTH", 0002);
  SetInt32Constant(env, binding, "S_IXOTH", 0001);
  SetInt32Constant(env, binding, "COPYFILE_EXCL", UV_FS_COPYFILE_EXCL);
  SetInt32Constant(env, binding, "COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE);
  SetInt32Constant(env, binding, "COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE);
  SetInt32Constant(env, binding, "UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL);
  SetInt32Constant(env, binding, "UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE);
  SetInt32Constant(env, binding, "UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE);
  SetInt32Constant(env, binding, "UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR);
  SetInt32Constant(env, binding, "UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION);

  return binding;
}

napi_value EdgeInstallFsDirBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) {
    return nullptr;
  }
  return binding;
}
