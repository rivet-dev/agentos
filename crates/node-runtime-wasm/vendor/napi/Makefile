.PHONY: build-napi test-napi test-napi-only build-napi-quickjs test-napi-quickjs test-napi-quickjs-only build-wasix-napi test-wasix-napi build-wasix-napi-quickjs test-wasix-napi-quickjs build-native-v8 test-native-v8 build-native-quickjs test-native-quickjs build-wasix-v8 test-wasix-v8 build-wasix-quickjs clean

UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
CMAKE_BUILD_TYPE ?= Release
JOBS ?= 8
TEST_JOBS ?= 0
NAPI_PROJECT_ROOT ?= $(abspath .)
BUILD_NAPI_DIR ?= $(NAPI_PROJECT_ROOT)/build-napi-v8
BUILD_NAPI_QUICKJS_DIR ?= $(NAPI_PROJECT_ROOT)/build-napi-quickjs
BUILD_WASIX_NAPI_DIR ?= $(NAPI_PROJECT_ROOT)/build-wasix-napi
BUILD_WASIX_NAPI_QUICKJS_DIR ?= $(NAPI_PROJECT_ROOT)/build-wasix-napi-quickjs
CMAKE_ARGS ?=
EXTRA_CMAKE_ARGS ?=
BUILD_ENV ?= env
WASIX_CMAKE_TOOLCHAIN ?= cmake/wasix-toolchain.cmake
CARGO_TARGET_DIR ?= $(abspath $(BUILD_WASIX_NAPI_DIR)/target)
NAPI_NATIVE_TEST_OUT_DIR ?= $(abspath $(BUILD_WASIX_NAPI_DIR)/native)
NAPI_WASIX_TEST_OUT_DIR ?= $(abspath $(BUILD_WASIX_NAPI_DIR)/wasm32-wasix/release)
NAPI_V8_CTEST_ARGS ?= -E 'SandboxGlobalThisAndMarkerAreNotEnumerableForDeepFreeze'
NAPI_QUICKJS_CTEST_ARGS ?= -E 'SandboxGlobalThisAndMarkerAreNotEnumerableForDeepFreeze'
NAPI_V8_PREBUILT_VERSION ?= 11.9.2
NAPI_V8_PLATFORM :=
NAPI_V8_DIST_ROOT ?=
NAPI_V8_CARGO_DIST_ROOT ?=
NAPI_V8_CMAKE_ARGS ?=

ifeq ($(UNAME_S),Darwin)
BUILD_ENV := env -u CPPFLAGS -u LDFLAGS
endif
ifeq ($(UNAME_S),Darwin)
ifeq ($(UNAME_M),arm64)
NAPI_V8_PLATFORM := darwin-arm64
else ifeq ($(UNAME_M),x86_64)
NAPI_V8_PLATFORM := darwin-amd64
endif
else ifeq ($(UNAME_S),Linux)
ifeq ($(UNAME_M),x86_64)
NAPI_V8_PLATFORM := linux-amd64
endif
endif
ifeq ($(NAPI_V8_DIST_ROOT),)
NAPI_V8_CARGO_DIST_ROOT := $(lastword $(sort $(wildcard $(abspath target/debug/build)/wasmer-napi-*/out/v8-prebuilt/$(NAPI_V8_PREBUILT_VERSION)/$(NAPI_V8_PLATFORM))))
ifneq ($(NAPI_V8_CARGO_DIST_ROOT),)
NAPI_V8_DIST_ROOT := $(NAPI_V8_CARGO_DIST_ROOT)
else
NAPI_V8_DIST_ROOT := $(BUILD_NAPI_DIR)/_v8_cache/$(NAPI_V8_PREBUILT_VERSION)/$(NAPI_V8_PLATFORM)
endif
endif
ifneq ($(NAPI_V8_PLATFORM),)
ifneq ($(wildcard $(NAPI_V8_DIST_ROOT)/include/v8.h),)
ifneq ($(wildcard $(NAPI_V8_DIST_ROOT)/lib/libv8.a),)
NAPI_V8_CMAKE_ARGS += -DNAPI_V8_BUILD_METHOD=local
NAPI_V8_CMAKE_ARGS += -DNAPI_V8_INCLUDE_DIR=$(NAPI_V8_DIST_ROOT)/include
NAPI_V8_CMAKE_ARGS += -DNAPI_V8_LIBRARY=$(NAPI_V8_DIST_ROOT)/lib/libv8.a
NAPI_V8_CMAKE_ARGS += -DNAPI_V8_DEFINES=V8_COMPRESS_POINTERS
ifeq ($(UNAME_S),Darwin)
NAPI_V8_CMAKE_ARGS += -DNAPI_V8_EXTRA_LIBS=/System/Library/Frameworks/CoreFoundation.framework
endif
endif
endif
endif

build-napi:
	$(BUILD_ENV) cmake -S . -B $(BUILD_NAPI_DIR) -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE) -DNAPI_PROJECT_ROOT=$(NAPI_PROJECT_ROOT) -DNAPI_BUILD_V8=ON -DNAPI_BUILD_QUICKJS=OFF $(NAPI_V8_CMAKE_ARGS) $(EXTRA_CMAKE_ARGS) $(CMAKE_ARGS)
	$(BUILD_ENV) cmake --build $(BUILD_NAPI_DIR) -j$(JOBS)

test-napi: build-napi test-napi-only

test-napi-only:
	$(BUILD_ENV) ctest --test-dir $(BUILD_NAPI_DIR) --output-on-failure -R '^napi_v8\.' $(NAPI_V8_CTEST_ARGS)

build-napi-quickjs:
	$(BUILD_ENV) cmake -S . -B $(BUILD_NAPI_QUICKJS_DIR) -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE) -DNAPI_PROJECT_ROOT=$(NAPI_PROJECT_ROOT) -DNAPI_BUILD_QUICKJS=ON -DNAPI_BUILD_V8=OFF $(EXTRA_CMAKE_ARGS) $(CMAKE_ARGS)
	$(BUILD_ENV) cmake --build $(BUILD_NAPI_QUICKJS_DIR) -j$(JOBS)

test-napi-quickjs: build-napi-quickjs test-napi-quickjs-only

test-napi-quickjs-only:
	$(BUILD_ENV) ctest --test-dir $(BUILD_NAPI_QUICKJS_DIR) --output-on-failure -R '^napi_quickjs\.' $(NAPI_QUICKJS_CTEST_ARGS)

build-wasix-napi:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" ./cargo-standalone.sh build --features cli --bin napi_wasmer
	@printf '%s\n' "Built $(CARGO_TARGET_DIR)/debug/napi_wasmer for WASIX N-API-import tests with the V8 host provider."

test-wasix-napi:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" NAPI_NATIVE_TEST_OUT_DIR="$(NAPI_NATIVE_TEST_OUT_DIR)" NAPI_WASIX_TEST_OUT_DIR="$(NAPI_WASIX_TEST_OUT_DIR)" ./cargo-standalone.sh test --features cli --test manifest_tests -- --nocapture

test-wasix-napi-quickjs: build-wasix-napi-quickjs

build-native-v8: build-napi

test-native-v8: test-napi

build-native-quickjs: build-napi-quickjs

test-native-quickjs: test-napi-quickjs

build-wasix-v8: build-wasix-napi

test-wasix-v8: test-wasix-napi

build-wasix-quickjs: build-wasix-napi-quickjs

clean:
	find . -maxdepth 1 -type d -name 'build-*' -exec rm -rf {} +
