#ifdef NAPI_MODULE
#undef NAPI_MODULE
#endif
#include "node_api.h"

#include "test_env.h"
#include "upstream_js_test.h"

#include <cstdint>
#include <cstring>

extern "C" napi_value Init(napi_env env, napi_value exports);

class Test21General : public FixtureTestBase {};

TEST_F(Test21General, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  napi_value addon = Init(s.env, exports);
  ASSERT_NE(addon, nullptr);
  ASSERT_TRUE(InstallUpstreamJsShim(s, addon));
  ASSERT_TRUE(
      RunUpstreamJsFile(s, std::string(NAPI_TESTS_ROOT_PATH) + "/js-native-api/test_general/test.js"));
}

#ifdef NAPI_TEST_ENGINE_QUICKJS
TEST_F(Test21General, GlobalBufferPrototypeDetection) {
  EnvScope s(runtime_.get());
  ASSERT_TRUE(RunScript(s, R"JS(
class FastBuffer extends Uint8Array {}
globalThis.FastBuffer = FastBuffer;
globalThis.bufferFromString = new FastBuffer([97, 98, 99]);
const backing = new ArrayBuffer(8);
new Uint8Array(backing).set([1, 2, 3], 2);
globalThis.slicedBuffer = new FastBuffer(backing, 2, 3);
globalThis.emptyBuffer = new FastBuffer();
globalThis.plainUint8Array = new Uint8Array([4, 5, 6]);
globalThis.plainArrayBuffer = new ArrayBuffer(4);
)JS",
                        "buffer-branding-setup"));

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);

  napi_value fast_buffer = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "FastBuffer", &fast_buffer), napi_ok);
  napi_value fast_buffer_prototype = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, fast_buffer, "prototype", &fast_buffer_prototype),
            napi_ok);

  napi_value initially_unbranded = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "bufferFromString", &initially_unbranded),
            napi_ok);
  bool initially_is_buffer = true;
  ASSERT_EQ(napi_is_buffer(s.env, initially_unbranded, &initially_is_buffer), napi_ok);
  EXPECT_FALSE(initially_is_buffer);

  ASSERT_TRUE(RunScript(s, R"JS(
function Buffer() {}
Buffer.prototype = FastBuffer.prototype;
globalThis.Buffer = Buffer;
)JS",
                        "buffer-global-setup"));

  auto expect_buffer_bytes = [&](const char* name, const uint8_t* expected, size_t length) {
    napi_value value = nullptr;
    ASSERT_EQ(napi_get_named_property(s.env, global, name, &value), napi_ok) << name;
    bool is_buffer = false;
    ASSERT_EQ(napi_is_buffer(s.env, value, &is_buffer), napi_ok) << name;
    EXPECT_TRUE(is_buffer) << name;

    void* raw = nullptr;
    size_t raw_length = 0;
    ASSERT_EQ(napi_get_buffer_info(s.env, value, &raw, &raw_length), napi_ok) << name;
    ASSERT_EQ(raw_length, length) << name;
    if (length > 0) {
      ASSERT_NE(raw, nullptr) << name;
      EXPECT_EQ(std::memcmp(raw, expected, length), 0) << name;
    }
  };

  const uint8_t abc[] = {97, 98, 99};
  expect_buffer_bytes("bufferFromString", abc, sizeof(abc));
  const uint8_t sliced[] = {1, 2, 3};
  expect_buffer_bytes("slicedBuffer", sliced, sizeof(sliced));
  expect_buffer_bytes("emptyBuffer", nullptr, 0);

  bool prototype_is_buffer = true;
  ASSERT_EQ(napi_is_buffer(s.env, fast_buffer_prototype, &prototype_is_buffer), napi_ok);
  EXPECT_FALSE(prototype_is_buffer);

  napi_value plain_uint8 = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "plainUint8Array", &plain_uint8), napi_ok);
  bool plain_uint8_is_buffer = true;
  ASSERT_EQ(napi_is_buffer(s.env, plain_uint8, &plain_uint8_is_buffer), napi_ok);
  EXPECT_FALSE(plain_uint8_is_buffer);
  void* raw = nullptr;
  size_t raw_length = 0;
  EXPECT_EQ(napi_get_buffer_info(s.env, plain_uint8, &raw, &raw_length), napi_invalid_arg);

  napi_value plain_arraybuffer = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "plainArrayBuffer", &plain_arraybuffer),
            napi_ok);
  bool plain_arraybuffer_is_buffer = true;
  ASSERT_EQ(napi_is_buffer(s.env, plain_arraybuffer, &plain_arraybuffer_is_buffer), napi_ok);
  EXPECT_FALSE(plain_arraybuffer_is_buffer);

  ASSERT_TRUE(RunScript(s, R"JS(
function FakeBuffer() {}
FakeBuffer.prototype = Uint8Array.prototype;
globalThis.Buffer = FakeBuffer;
globalThis.fakePlainUint8Array = new Uint8Array([9, 10, 11]);
)JS",
                        "buffer-global-replacement"));
  expect_buffer_bytes("bufferFromString", abc, sizeof(abc));
  napi_value fake_plain_uint8 = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "fakePlainUint8Array", &fake_plain_uint8),
            napi_ok);
  bool fake_plain_uint8_is_buffer = true;
  ASSERT_EQ(napi_is_buffer(s.env, fake_plain_uint8, &fake_plain_uint8_is_buffer), napi_ok);
  EXPECT_FALSE(fake_plain_uint8_is_buffer);
}

TEST_F(Test21General, NativeCreatedBuffersAdoptGlobalBufferPrototype) {
  EnvScope s(runtime_.get());
  ASSERT_TRUE(RunScript(s, R"JS(
class FastBuffer extends Uint8Array {}
globalThis.FastBuffer = FastBuffer;
)JS",
                        "native-buffer-setup"));

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  napi_value fast_buffer = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "FastBuffer", &fast_buffer), napi_ok);

  auto expect_native_buffer_bytes = [&](napi_value value, const uint8_t* expected, size_t length) {
    bool is_buffer = false;
    ASSERT_EQ(napi_is_buffer(s.env, value, &is_buffer), napi_ok);
    EXPECT_TRUE(is_buffer);

    void* raw = nullptr;
    size_t raw_length = 0;
    ASSERT_EQ(napi_get_buffer_info(s.env, value, &raw, &raw_length), napi_ok);
    ASSERT_EQ(raw_length, length);
    if (length > 0) {
      ASSERT_NE(raw, nullptr);
      EXPECT_EQ(std::memcmp(raw, expected, length), 0);
    }
  };

  const uint8_t before_bytes[] = {7, 8, 9};
  napi_value native_before = nullptr;
  ASSERT_EQ(napi_create_buffer_copy(s.env,
                                    sizeof(before_bytes),
                                    before_bytes,
                                    nullptr,
                                    &native_before),
            napi_ok);
  expect_native_buffer_bytes(native_before, before_bytes, sizeof(before_bytes));

  ASSERT_TRUE(RunScript(s, R"JS(
function Buffer() {}
Buffer.prototype = FastBuffer.prototype;
globalThis.Buffer = Buffer;
)JS",
                        "native-buffer-global-setup"));
  expect_native_buffer_bytes(native_before, before_bytes, sizeof(before_bytes));
  bool native_before_instanceof = false;
  ASSERT_EQ(napi_instanceof(s.env, native_before, fast_buffer, &native_before_instanceof),
            napi_ok);
  EXPECT_TRUE(native_before_instanceof);

  const uint8_t after_bytes[] = {10, 11, 12};
  napi_value native_after = nullptr;
  ASSERT_EQ(napi_create_buffer_copy(s.env,
                                    sizeof(after_bytes),
                                    after_bytes,
                                    nullptr,
                                    &native_after),
            napi_ok);
  expect_native_buffer_bytes(native_after, after_bytes, sizeof(after_bytes));
  bool native_after_instanceof = false;
  ASSERT_EQ(napi_instanceof(s.env, native_after, fast_buffer, &native_after_instanceof),
            napi_ok);
  EXPECT_TRUE(native_after_instanceof);
}
#endif
