#include "internal_binding/binding_initializers.h"

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <limits>
#include <string>
#include <utility>
#include <vector>

#include "edge_environment.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

constexpr const char* kBlobDataKey = "__edge_blob_data";
constexpr const char* kBlobChunksKey = "__edge_blob_chunks";
constexpr const char* kBlobReaderDoneKey = "__edge_blob_reader_done";
constexpr const char* kBlobReaderIndexKey = "__edge_blob_reader_index";

struct StoredDataObject {
  napi_ref handle_ref = nullptr;
  uint32_t length = 0;
  std::string type;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

struct BlobBindingState {
  explicit BlobBindingState(napi_env env_in) : env(env_in) {}
  ~BlobBindingState() {
    for (auto& entry : objects) {
      DeleteRefIfPresent(env, &entry.second.handle_ref);
    }
    objects.clear();
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &handle_slice_ref);
    DeleteRefIfPresent(env, &handle_get_reader_ref);
    DeleteRefIfPresent(env, &reader_pull_ref);
  }

  napi_env env = nullptr;
  std::unordered_map<std::string, StoredDataObject> objects;
  napi_ref binding_ref = nullptr;
  napi_ref handle_slice_ref = nullptr;
  napi_ref handle_get_reader_ref = nullptr;
  napi_ref reader_pull_ref = nullptr;
};

BlobBindingState* GetBlobState(napi_env env) {
  return EdgeEnvironmentGetSlotData<BlobBindingState>(env, kEdgeEnvironmentSlotBlobBindingState);
}

BlobBindingState& EnsureBlobState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<BlobBindingState>(
      env, kEdgeEnvironmentSlotBlobBindingState);
}

bool GetNamedProperty(napi_env env, napi_value obj, const char* key, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (obj == nullptr) return false;
  bool has = false;
  if (napi_has_named_property(env, obj, key, &has) != napi_ok || !has) return false;
  return napi_get_named_property(env, obj, key, out) == napi_ok && *out != nullptr;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) {
    return nullptr;
  }
  return value;
}

bool ValueToUtf8(napi_env env, napi_value value, std::string* out) {
  if (out == nullptr || value == nullptr) return false;
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) return false;
  std::string tmp(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, tmp.data(), tmp.size(), &copied) != napi_ok) return false;
  tmp.resize(copied);
  *out = std::move(tmp);
  return true;
}

size_t TypedArrayElementSize(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
    case napi_uint8_array:
    case napi_uint8_clamped_array:
      return 1;
    case napi_int16_array:
    case napi_uint16_array:
      return 2;
    case napi_int32_array:
    case napi_uint32_array:
    case napi_float32_array:
      return 4;
    case napi_float64_array:
    case napi_bigint64_array:
    case napi_biguint64_array:
      return 8;
    default:
      return 1;
  }
}

bool AppendRawBytes(const uint8_t* data, size_t length, std::vector<uint8_t>* out) {
  if (out == nullptr) return false;
  if (data == nullptr || length == 0) return true;
  out->insert(out->end(), data, data + length);
  return true;
}

bool AppendBytesFromValue(napi_env env, napi_value value, std::vector<uint8_t>* out) {
  if (value == nullptr || out == nullptr) return false;

  // Blob handle objects created by this binding store their payload here.
  napi_value blob_data = nullptr;
  if (GetNamedProperty(env, value, kBlobDataKey, &blob_data)) {
    return AppendBytesFromValue(env, blob_data, out);
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* data = nullptr;
    size_t length = 0;
    if (napi_get_arraybuffer_info(env, value, &data, &length) != napi_ok) return false;
    return AppendRawBytes(static_cast<const uint8_t*>(data), length, out);
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    napi_value arraybuffer = nullptr;
    size_t offset = 0;
    if (napi_get_typedarray_info(
            env, value, &type, &length, &data, &arraybuffer, &offset) != napi_ok) {
      return false;
    }
    (void)arraybuffer;
    (void)offset;
    const size_t byte_length = length * TypedArrayElementSize(type);
    return AppendRawBytes(static_cast<const uint8_t*>(data), byte_length, out);
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_length = 0;
    void* data = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(
            env, value, &byte_length, &data, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }
    (void)arraybuffer;
    (void)byte_offset;
    return AppendRawBytes(static_cast<const uint8_t*>(data), byte_length, out);
  }

  return false;
}

napi_value CreateArrayBufferFromBytes(napi_env env, const uint8_t* bytes, size_t length) {
  void* data = nullptr;
  napi_value out = nullptr;
  if (napi_create_arraybuffer(env, length, &data, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  if (length > 0 && data != nullptr && bytes != nullptr) {
    std::memcpy(data, bytes, length);
  }
  return out;
}

napi_value CreateArrayBufferFromBytes(napi_env env, const std::vector<uint8_t>& bytes) {
  return CreateArrayBufferFromBytes(env, bytes.data(), bytes.size());
}

napi_value BlobReaderPullCallback(napi_env env, napi_callback_info info);
napi_value BlobHandleSliceCallback(napi_env env, napi_callback_info info);
napi_value BlobHandleGetReaderCallback(napi_env env, napi_callback_info info);

napi_value GetOrCreateCachedFunction(napi_env env,
                                     napi_ref* slot,
                                     const char* name,
                                     napi_callback callback) {
  if (slot == nullptr || callback == nullptr) return nullptr;

  napi_value cached = GetRefValue(env, *slot);
  if (cached != nullptr) return cached;

  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, callback, nullptr, &fn) != napi_ok ||
      fn == nullptr) {
    return nullptr;
  }

  DeleteRefIfPresent(env, slot);
  if (napi_create_reference(env, fn, 1, slot) != napi_ok) {
    return fn;
  }
  return fn;
}

napi_value CreateBlobHandle(napi_env env, napi_value data_arraybuffer, napi_value chunks = nullptr) {
  BlobBindingState& state = EnsureBlobState(env);

  napi_value handle = nullptr;
  if (napi_create_object(env, &handle) != napi_ok || handle == nullptr) return Undefined(env);
  if (napi_set_named_property(env, handle, kBlobDataKey, data_arraybuffer) != napi_ok) {
    return Undefined(env);
  }

  if (chunks == nullptr) {
    if (napi_create_array_with_length(env, 1, &chunks) != napi_ok || chunks == nullptr ||
        napi_set_element(env, chunks, 0, data_arraybuffer) != napi_ok) {
      return Undefined(env);
    }
  }
  if (napi_set_named_property(env, handle, kBlobChunksKey, chunks) != napi_ok) {
    return Undefined(env);
  }

  napi_value slice_fn = GetOrCreateCachedFunction(
      env, &state.handle_slice_ref, "slice", BlobHandleSliceCallback);
  if (slice_fn == nullptr ||
      napi_set_named_property(env, handle, "slice", slice_fn) != napi_ok) {
    return Undefined(env);
  }

  napi_value get_reader_fn = GetOrCreateCachedFunction(
      env, &state.handle_get_reader_ref, "getReader", BlobHandleGetReaderCallback);
  if (get_reader_fn == nullptr ||
      napi_set_named_property(env, handle, "getReader", get_reader_fn) != napi_ok) {
    return Undefined(env);
  }

  return handle;
}

napi_value CreateBlobHandleFromBytes(napi_env env, const std::vector<uint8_t>& bytes) {
  napi_value ab = CreateArrayBufferFromBytes(env, bytes);
  if (ab == nullptr || IsUndefined(env, ab)) return Undefined(env);
  return CreateBlobHandle(env, ab);
}

napi_value CreateBlobHandleFromBytesAndChunks(napi_env env,
                                              const std::vector<uint8_t>& bytes,
                                              napi_value chunks) {
  napi_value ab = CreateArrayBufferFromBytes(env, bytes);
  if (ab == nullptr || IsUndefined(env, ab)) return Undefined(env);
  return CreateBlobHandle(env, ab, chunks);
}

bool GetBlobHandleArrayBuffer(napi_env env, napi_value handle, napi_value* out_arraybuffer) {
  if (out_arraybuffer == nullptr) return false;
  *out_arraybuffer = nullptr;
  napi_value arraybuffer = nullptr;
  if (!GetNamedProperty(env, handle, kBlobDataKey, &arraybuffer)) return false;
  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, arraybuffer, &is_arraybuffer) != napi_ok || !is_arraybuffer) {
    return false;
  }
  *out_arraybuffer = arraybuffer;
  return true;
}

napi_value BlobHandleSliceCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return Undefined(env);
  }

  napi_value arraybuffer = nullptr;
  if (!GetBlobHandleArrayBuffer(env, this_arg, &arraybuffer)) return Undefined(env);

  void* data = nullptr;
  size_t byte_length = 0;
  if (napi_get_arraybuffer_info(env, arraybuffer, &data, &byte_length) != napi_ok) {
    return Undefined(env);
  }

  uint32_t start = 0;
  uint32_t end = static_cast<uint32_t>(std::min<size_t>(byte_length, std::numeric_limits<uint32_t>::max()));
  if (argc >= 1 && argv[0] != nullptr) {
    (void)napi_get_value_uint32(env, argv[0], &start);
  }
  if (argc >= 2 && argv[1] != nullptr) {
    (void)napi_get_value_uint32(env, argv[1], &end);
  }

  const uint32_t length_u32 =
      static_cast<uint32_t>(std::min<size_t>(byte_length, std::numeric_limits<uint32_t>::max()));
  if (start > length_u32) start = length_u32;
  if (end > length_u32) end = length_u32;
  if (end < start) end = start;

  const uint8_t* bytes = static_cast<const uint8_t*>(data);
  const size_t span = static_cast<size_t>(end - start);
  napi_value sliced = CreateArrayBufferFromBytes(env, bytes + start, span);
  if (sliced == nullptr || IsUndefined(env, sliced)) return Undefined(env);
  return CreateBlobHandle(env, sliced);
}

napi_value BlobHandleGetReaderCallback(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return Undefined(env);
  }

  napi_value arraybuffer = nullptr;
  if (!GetBlobHandleArrayBuffer(env, this_arg, &arraybuffer)) return Undefined(env);
  napi_value chunks = nullptr;
  if (!GetNamedProperty(env, this_arg, kBlobChunksKey, &chunks)) {
    if (napi_create_array_with_length(env, 1, &chunks) != napi_ok || chunks == nullptr ||
        napi_set_element(env, chunks, 0, arraybuffer) != napi_ok) {
      return Undefined(env);
    }
  }

  napi_value reader = nullptr;
  if (napi_create_object(env, &reader) != napi_ok || reader == nullptr) return Undefined(env);
  if (napi_set_named_property(env, reader, kBlobDataKey, arraybuffer) != napi_ok) return Undefined(env);
  if (napi_set_named_property(env, reader, kBlobChunksKey, chunks) != napi_ok) return Undefined(env);

  napi_value done = nullptr;
  if (napi_get_boolean(env, false, &done) != napi_ok || done == nullptr) return Undefined(env);
  if (napi_set_named_property(env, reader, kBlobReaderDoneKey, done) != napi_ok) return Undefined(env);
  napi_value index = nullptr;
  if (napi_create_uint32(env, 0, &index) != napi_ok || index == nullptr) return Undefined(env);
  if (napi_set_named_property(env, reader, kBlobReaderIndexKey, index) != napi_ok) return Undefined(env);

  BlobBindingState& state = EnsureBlobState(env);
  napi_value pull_fn = GetOrCreateCachedFunction(
      env, &state.reader_pull_ref, "pull", BlobReaderPullCallback);
  if (pull_fn == nullptr ||
      napi_set_named_property(env, reader, "pull", pull_fn) != napi_ok) {
    return Undefined(env);
  }

  return reader;
}

napi_value BlobReaderPullCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok ||
      this_arg == nullptr ||
      argc < 1 ||
      argv[0] == nullptr) {
    return Undefined(env);
  }

  napi_valuetype cb_type = napi_undefined;
  if (napi_typeof(env, argv[0], &cb_type) != napi_ok || cb_type != napi_function) {
    return Undefined(env);
  }
  napi_value callback = argv[0];

  bool done = false;
  napi_value done_value = nullptr;
  if (GetNamedProperty(env, this_arg, kBlobReaderDoneKey, &done_value)) {
    (void)napi_get_value_bool(env, done_value, &done);
  }

  napi_value chunks = nullptr;
  if (!GetNamedProperty(env, this_arg, kBlobChunksKey, &chunks)) return Undefined(env);

  int32_t status = 0;
  napi_value maybe_data = Undefined(env);
  if (!done) {
    uint32_t index = 0;
    uint32_t chunk_count = 0;
    napi_value index_value = nullptr;
    if (GetNamedProperty(env, this_arg, kBlobReaderIndexKey, &index_value)) {
      (void)napi_get_value_uint32(env, index_value, &index);
    }
    (void)napi_get_array_length(env, chunks, &chunk_count);

    if (index < chunk_count &&
        napi_get_element(env, chunks, index, &maybe_data) == napi_ok &&
        maybe_data != nullptr) {
      status = 1;

      napi_value next_index = nullptr;
      if (napi_create_uint32(env, index + 1, &next_index) == napi_ok && next_index != nullptr) {
        napi_set_named_property(env, this_arg, kBlobReaderIndexKey, next_index);
      }
    } else {
      status = 0;
      maybe_data = Undefined(env);
      done = true;
    }

    if (index + 1 >= chunk_count || done) {
      napi_value done_true = nullptr;
      if (napi_get_boolean(env, true, &done_true) == napi_ok && done_true != nullptr) {
        napi_set_named_property(env, this_arg, kBlobReaderDoneKey, done_true);
      }
    }
  }

  napi_value global = GetGlobal(env);
  if (global == nullptr) global = Undefined(env);

  napi_value status_value = nullptr;
  if (napi_create_int32(env, status, &status_value) != napi_ok || status_value == nullptr) {
    return Undefined(env);
  }

  napi_value cb_argv[2] = {status_value, maybe_data};
  napi_value ignored = nullptr;
  if (napi_call_function(env, global, callback, 2, cb_argv, &ignored) != napi_ok) {
    return Undefined(env);
  }
  return status_value;
}

napi_value BlobConcatCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  bool is_array = false;
  if (napi_is_array(env, argv[0], &is_array) != napi_ok || !is_array) return Undefined(env);

  uint32_t length = 0;
  if (napi_get_array_length(env, argv[0], &length) != napi_ok) return Undefined(env);

  std::vector<uint8_t> bytes;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value element = nullptr;
    if (napi_get_element(env, argv[0], i, &element) != napi_ok || element == nullptr) continue;
    (void)AppendBytesFromValue(env, element, &bytes);
  }

  return CreateArrayBufferFromBytes(env, bytes);
}

napi_value BlobCreateBlobCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  bool is_array = false;
  if (napi_is_array(env, argv[0], &is_array) != napi_ok || !is_array) return Undefined(env);

  std::vector<uint8_t> bytes;
  napi_value chunks = nullptr;
  if (napi_create_array_with_length(env, 0, &chunks) != napi_ok || chunks == nullptr) return Undefined(env);
  if (argc >= 2 && argv[1] != nullptr) {
    uint32_t expected = 0;
    if (napi_get_value_uint32(env, argv[1], &expected) == napi_ok) {
      bytes.reserve(expected);
    }
  }

  uint32_t source_count = 0;
  if (napi_get_array_length(env, argv[0], &source_count) != napi_ok) return Undefined(env);

  for (uint32_t i = 0; i < source_count; ++i) {
    napi_value source = nullptr;
    if (napi_get_element(env, argv[0], i, &source) != napi_ok || source == nullptr) continue;
    std::vector<uint8_t> source_bytes;
    if (!AppendBytesFromValue(env, source, &source_bytes)) continue;
    bytes.insert(bytes.end(), source_bytes.begin(), source_bytes.end());
    napi_value chunk = CreateArrayBufferFromBytes(env, source_bytes);
    if (chunk == nullptr || IsUndefined(env, chunk)) return Undefined(env);
    if (napi_set_element(env, chunks, i, chunk) != napi_ok) return Undefined(env);
  }

  return CreateBlobHandleFromBytesAndChunks(env, bytes, chunks);
}

napi_value BlobCreateBlobFromFilePathCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  std::string path;
  if (!ValueToUtf8(env, argv[0], &path) || path.empty()) return Undefined(env);

  std::ifstream in(path, std::ios::binary);
  if (!in.is_open()) return Undefined(env);

  std::vector<uint8_t> bytes;
  in.seekg(0, std::ios::end);
  const std::streamoff file_size = in.tellg();
  in.seekg(0, std::ios::beg);
  if (file_size < 0) return Undefined(env);
  if (file_size > 0) {
    bytes.resize(static_cast<size_t>(file_size));
    in.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    if (!in.good() && !in.eof()) return Undefined(env);
  }

  napi_value handle = CreateBlobHandleFromBytes(env, bytes);
  if (handle == nullptr || IsUndefined(env, handle)) return Undefined(env);

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 2, &out) != napi_ok || out == nullptr) return Undefined(env);
  if (napi_set_element(env, out, 0, handle) != napi_ok) return Undefined(env);

  const uint32_t length =
      bytes.size() > std::numeric_limits<uint32_t>::max()
          ? std::numeric_limits<uint32_t>::max()
          : static_cast<uint32_t>(bytes.size());
  napi_value length_value = nullptr;
  if (napi_create_uint32(env, length, &length_value) != napi_ok || length_value == nullptr) {
    return Undefined(env);
  }
  if (napi_set_element(env, out, 1, length_value) != napi_ok) return Undefined(env);
  return out;
}

napi_value BlobStoreDataObjectCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) {
    return Undefined(env);
  }

  std::string key;
  if (!ValueToUtf8(env, argv[0], &key) || key.empty()) return Undefined(env);

  uint32_t length = 0;
  (void)napi_get_value_uint32(env, argv[2], &length);

  std::string type;
  (void)ValueToUtf8(env, argv[3], &type);

  napi_ref handle_ref = nullptr;
  if (napi_create_reference(env, argv[1], 1, &handle_ref) != napi_ok || handle_ref == nullptr) {
    return Undefined(env);
  }

  BlobBindingState& state = EnsureBlobState(env);
  auto existing = state.objects.find(key);
  if (existing != state.objects.end()) {
    if (existing->second.handle_ref != nullptr) {
      DeleteRefIfPresent(env, &existing->second.handle_ref);
    }
    state.objects.erase(existing);
  }
  state.objects.emplace(key, StoredDataObject{handle_ref, length, type});
  return Undefined(env);
}

napi_value BlobGetDataObjectCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  std::string key;
  if (!ValueToUtf8(env, argv[0], &key) || key.empty()) return Undefined(env);

  auto* state = GetBlobState(env);
  if (state == nullptr) return Undefined(env);
  auto item_it = state->objects.find(key);
  if (item_it == state->objects.end()) return Undefined(env);

  napi_value handle = nullptr;
  if (napi_get_reference_value(env, item_it->second.handle_ref, &handle) != napi_ok || handle == nullptr) {
    return Undefined(env);
  }

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 3, &out) != napi_ok || out == nullptr) return Undefined(env);
  if (napi_set_element(env, out, 0, handle) != napi_ok) return Undefined(env);

  napi_value length_value = nullptr;
  if (napi_create_uint32(env, item_it->second.length, &length_value) != napi_ok || length_value == nullptr) {
    return Undefined(env);
  }
  if (napi_set_element(env, out, 1, length_value) != napi_ok) return Undefined(env);

  napi_value type_value = nullptr;
  if (napi_create_string_utf8(
          env, item_it->second.type.c_str(), item_it->second.type.size(), &type_value) != napi_ok ||
      type_value == nullptr) {
    return Undefined(env);
  }
  if (napi_set_element(env, out, 2, type_value) != napi_ok) return Undefined(env);
  return out;
}

napi_value BlobRevokeObjectURLCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  std::string key;
  if (!ValueToUtf8(env, argv[0], &key) || key.empty()) return Undefined(env);

  auto* state = GetBlobState(env);
  if (state == nullptr) return Undefined(env);
  auto item_it = state->objects.find(key);
  if (item_it == state->objects.end()) return Undefined(env);
  DeleteRefIfPresent(env, &item_it->second.handle_ref);
  state->objects.erase(item_it);
  return Undefined(env);
}

bool DefineMethod(napi_env env, napi_value target, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) != napi_ok || fn == nullptr) {
    return false;
  }
  return napi_set_named_property(env, target, name, fn) == napi_ok;
}

}  // namespace

napi_value InitBlob(napi_env env) {
  auto* existing_state = GetBlobState(env);
  if (existing_state != nullptr && existing_state->binding_ref != nullptr) {
    napi_value cached = nullptr;
    if (napi_get_reference_value(env, existing_state->binding_ref, &cached) == napi_ok &&
        cached != nullptr) {
      return cached;
    }
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  if (!DefineMethod(env, binding, "createBlob", BlobCreateBlobCallback) ||
      !DefineMethod(env, binding, "storeDataObject", BlobStoreDataObjectCallback) ||
      !DefineMethod(env, binding, "getDataObject", BlobGetDataObjectCallback) ||
      !DefineMethod(env, binding, "revokeObjectURL", BlobRevokeObjectURLCallback) ||
      !DefineMethod(env, binding, "concat", BlobConcatCallback) ||
      !DefineMethod(env, binding, "createBlobFromFilePath", BlobCreateBlobFromFilePathCallback)) {
    return Undefined(env);
  }

  BlobBindingState& state = EnsureBlobState(env);
  DeleteRefIfPresent(env, &state.binding_ref);
  if (napi_create_reference(env, binding, 1, &state.binding_ref) != napi_ok) {
    state.binding_ref = nullptr;
  }

  return binding;
}

}  // namespace internal_binding
