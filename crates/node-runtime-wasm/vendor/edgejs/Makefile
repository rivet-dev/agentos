.PHONY: build build-edge build-edge-quickjs-cli build-wasix build-quickjs-wasix build-napi build-napi-quickjs build-native-v8 build-native-quickjs build-wasix-napi build-wasix-napi-quickjs build-napi-wasmer-cli test-wasix-napi test-wasix-napi-quickjs test-wasix-napi-cli test-wasix-safe-mode test-wasix-quickjs-only test-quickjs-intl test-wasix-quickjs-intl test test-only check-portability clean clean-napi-quickjs clean-edge-quickjs-cli clean-dist dist dist-only framework-test framework-test-quickjs-native framework-test-quickjs-wasix framework-test-run framework-test-reset standalone-build-test standalone-build-test-run standalone-build-test-quickjs-native standalone-build-test-quickjs-wasix

UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
BUILD_DIR ?= build-edge
BUILD_EDGE_QUICKJS_CLI_DIR ?= build-edge-quickjs-cli
BUILD_WASIX_NAPI_DIR ?= build-wasix-napi
BUILD_QUICKJS_WASIX_DIR ?= build-quickjs-wasix
QUICKJS_WASIX_WASM := $(BUILD_QUICKJS_WASIX_DIR)/edgejs.wasm
DIST_DIR ?= dist
DIST_BIN_DIR ?= $(DIST_DIR)/bin
DIST_BIN_COMPAT_DIR ?= $(DIST_DIR)/bin-compat
ZIP_NAME ?= edge.zip
CMAKE_BUILD_TYPE ?= Release
JOBS ?= 8
TEST_JOBS ?= 0
EDGE_BINARY ?= $(BUILD_DIR)/edge
EDGEENV_BINARY ?= $(BUILD_DIR)/edgeenv
CMAKE_ARGS ?=
BUILD_ENV ?= env
EXTRA_CMAKE_ARGS ?=
NAPI_V8_PREBUILT_VERSION ?= 11.9.2
NAPI_V8_PLATFORM :=
FRAMEWORK_TEST_SCRIPT := $(CURDIR)/scripts/framework-test.js
STANDALONE_BUILD_TEST_SCRIPT := $(CURDIR)/scripts/standalone-build-test.js
FRAMEWORK_TEST_SELECTOR := $(filter js-%,$(MAKECMDGOALS))
FRAMEWORK_TEST_ORCHESTRATOR ?= node
WASIX_FRAMEWORK_RUNNER := $(CURDIR)/scripts/edge-wasix-framework-runner.sh
QUICKJS_EDGE_BINARY := $(BUILD_EDGE_QUICKJS_CLI_DIR)/edge
NAPI_WASMER_DIR ?= napi
NAPI_WASMER_CARGO_TARGET_DIR ?= $(abspath $(BUILD_WASIX_NAPI_DIR)/target)
NAPI_WASMER_BINARY ?= $(NAPI_WASMER_CARGO_TARGET_DIR)/debug/napi_wasmer
WASIX_EDGEJS_WASM ?= ./build-wasix/edgejs.wasm
WASIX_NAPI_SMOKE_JS ?= console.log('hello world!');
WASMER_BIN ?= wasmer
WASIX_PACKAGE_DIR ?= $(CURDIR)
WASIX_SSL_CERTS_DIR ?= ssl-certs
WASIX_QUICKJS_NODE_TEST_RUNNER ?= $(CURDIR)/scripts/edge-wasix-node-runner.sh
EDGE_VERSION_MAJOR := $(shell awk '$$2 == "EDGE_MAJOR_VERSION" {print $$3; exit}' src/edge_version.h)
EDGE_VERSION_MINOR := $(shell awk '$$2 == "EDGE_MINOR_VERSION" {print $$3; exit}' src/edge_version.h)
EDGE_VERSION_PATCH := $(shell awk '$$2 == "EDGE_PATCH_VERSION" {print $$3; exit}' src/edge_version.h)
EDGE_VERSION_COMMIT := $(shell git rev-parse --short=7 HEAD 2>/dev/null || printf unknown)
EDGE_VERSION_BASE := $(EDGE_VERSION_MAJOR).$(EDGE_VERSION_MINOR).$(EDGE_VERSION_PATCH)
ifneq ($(filter 1 true TRUE yes YES,$(IS_FINAL_RELEASE)),)
EDGE_PACKAGE_VERSION := $(EDGE_VERSION_BASE)
else
EDGE_PACKAGE_VERSION := $(EDGE_VERSION_BASE)-$(EDGE_VERSION_COMMIT)
endif
EDGE_WASMER_PACKAGE ?= wasmer/edgejs@=$(EDGE_PACKAGE_VERSION)

EMPTY :=
SPACE := $(EMPTY) $(EMPTY)
COMMA := ,

# Temporarily excluded from Edge's Node compatibility category lanes after the
# 2026-05-18 CI failure sweep. Keep this list deduped because both V8 and
# QuickJS targets share part of the failing set.
EDGE_NODE_TEST_SKIP_CI_20260518 := \
  abort/test-http-parser-consume.js \
  abort/test-zlib-invalid-internals-usage.js \
  parallel/test-dns-channel-timeout.js \
  parallel/test-domain-multiple-errors.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-0.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-1.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-2.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-3.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-4.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-5.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-6.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-7.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-8.js \
  parallel/test-domain-no-error-handler-abort-on-uncaught-9.js \
  parallel/test-domain-throw-error-then-throw-from-uncaught-exception-handler.js \
  parallel/test-domain-vm-promise-isolation.js \
  parallel/test-domain-with-abort-on-uncaught-exception.js \
  parallel/test-http-server-headers-timeout-keepalive.js \
  parallel/test-http-server-request-timeout-keepalive.js \
  parallel/test-http2-client-shutdown-before-connect.js \
  parallel/test-http2-forget-closed-streams.js \
  parallel/test-http2-max-settings.js \
  parallel/test-http2-pipe.js \
  parallel/test-http2-response-splitting.js \
  parallel/test-strace-openat-openssl.js \
  parallel/test-stream-pipeline.js \
  parallel/test-stream-readable-async-iterators.js \
  parallel/test-zlib-type-error.js \
  pseudo-tty/console_colors.js
# Additional intermittent CI failures quarantined 2026-07-03 (shared by both
# lanes). test-http-pipeline-requests-connection-leak is a throughput stress
# test (10k pipelined requests / ~160 MB of responses) that races the 10s
# per-test timeout under parallel CI load. (test-tls-alert-handling was also
# quarantined here for a macOS SIGSEGV; that shutdown-request use-after-free is
# fixed in this change, so it is re-enabled.)
EDGE_NODE_TEST_SKIP_CI_20260703 := \
  parallel/test-http-pipeline-requests-connection-leak.js
EDGE_NODE_TEST_SKIP_TESTS ?= $(subst $(SPACE),$(COMMA),$(strip $(EDGE_NODE_TEST_SKIP_CI_20260518) $(EDGE_NODE_TEST_SKIP_CI_20260703)))

# QuickJS currently cannot parse explicit resource management `using` syntax.
QUICKJS_SKIP_USING_PARSER_TESTS := parallel/test-stream-duplex-destroy.js,parallel/test-stream-readable-dispose.js,parallel/test-stream-transform-destroy.js,parallel/test-stream-writable-destroy.js
# QuickJS worker_threads/MessagePort support is incomplete; these worker-backed
# tests time out or fail in the QuickJS lane while V8 continues to cover them.
QUICKJS_SKIP_WORKER_TESTS := parallel/test-diagnostics-channel-worker-threads.js,client-proxy/test-http-proxy-request-invalid-char-in-url.mjs,parallel/test-crypto-key-objects-messageport.js,parallel/test-crypto-prime.js,parallel/test-crypto-worker-thread.js,parallel/test-http2-reset-flood.js,parallel/test-webcrypto-cryptokey-workers.js
# QuickJS currently regresses TLS close-notify handling under --expose-internals.
QUICKJS_SKIP_TLS_TESTS := parallel/test-tls-close-notify.js
QUICKJS_SKIP_TESTS ?= $(EDGE_NODE_TEST_SKIP_TESTS),$(QUICKJS_SKIP_USING_PARSER_TESTS),$(QUICKJS_SKIP_WORKER_TESTS),$(QUICKJS_SKIP_TLS_TESTS)

# Expected WASIX environment limits from the 2026-06-23 triage run (1674 passed /
# 64 failed). These 52 tests are unix sockets, cluster/fork, subprocess/shell,
# homedir/priority, stack-overflow console, UDP gaps, unsupported crypto, and
# TLS env/keylog subprocess harnesses. The 12 in-process parity targets below
# are tracked in plans/quickjs-wasm/development/010_wasix_remaining_node_test_failures.md.
WASIX_SKIP_UNIX_SOCKET_TESTS := \
  parallel/test-http-client-abort-keep-alive-queued-unix-socket.js \
  parallel/test-http-client-abort-unix-socket.js \
  parallel/test-http-client-pipe-end.js \
  parallel/test-http-unix-socket-keep-alive.js \
  parallel/test-http-unix-socket.js \
  parallel/test-https-unix-socket-self-signed.js \
  parallel/test-http2-pipe-named-pipe.js \
  parallel/test-http2-respond-file-error-pipe-offset.js \
  parallel/test-tls-connect-pipe.js \
  parallel/test-tls-net-connect-prefer-path.js \
  parallel/test-tls-wrap-econnreset-pipe.js \
  parallel/test-http-client-response-domain.js
WASIX_SKIP_CLUSTER_FORK_TESTS := \
  parallel/test-dgram-bind-socket-close-before-cluster-reply.js \
  parallel/test-dgram-cluster-close-during-bind.js \
  parallel/test-dgram-cluster-close-in-listening.js \
  parallel/test-dgram-unref-in-cluster.js \
  parallel/test-http-server-drop-connections-in-cluster.js \
  parallel/test-tls-ticket-cluster.js \
  parallel/test-diagnostics-channel-process.js \
  parallel/test-http-chunk-problem.js \
  parallel/test-http-client-with-create-connection.js \
  parallel/test-http-full-response.js \
  parallel/test-http-server-stale-close.js \
  parallel/test-dgram-deprecation-error.js \
  parallel/test-https-agent-unref-socket.js \
  parallel/test-crypto-secure-heap.js \
  parallel/test-domain-top-level-error-handler-throw.js \
  parallel/test-domain-uncaught-exception.js \
  sequential/test-dgram-bind-shared-ports.js
WASIX_SKIP_SUBPROCESS_SHELL_TESTS := \
  parallel/test-stream-pipeline-process.js \
  parallel/test-domain-abort-on-uncaught.js \
  sequential/test-stream2-stderr-sync.js
WASIX_SKIP_OS_TESTS := \
  parallel/test-os-homedir-no-envvar.js \
  parallel/test-os.js \
  parallel/test-os-process-priority.js
WASIX_SKIP_STACK_OVERFLOW_TESTS := \
  parallel/test-console-log-throw-primitive.js \
  parallel/test-console-no-swallow-stack-overflow.js \
  parallel/test-console-sync-write-error.js \
  parallel/test-ttywrap-stack.js
WASIX_SKIP_UDP_TESTS := \
  parallel/test-dgram-createSocket-type.js \
  parallel/test-dgram-exclusive-implicit-bind.js \
  parallel/test-dgram-setTTL.js
WASIX_SKIP_CRYPTO_UNSUPPORTED_TESTS := \
  parallel/test-crypto-argon2.js \
  parallel/test-crypto-no-algorithm.js \
  parallel/test-webcrypto-derivebits-argon2.js \
  parallel/test-crypto-pqc-keygen-slh-dsa.js
WASIX_SKIP_TLS_SUBPROCESS_ENV_TESTS := \
  parallel/test-tls-enable-keylog-cli.js \
  parallel/test-tls-env-bad-extra-ca.js \
  parallel/test-tls-env-extra-ca-no-crypto.js \
  parallel/test-tls-env-extra-ca.js \
  parallel/test-tls-env-extra-ca-with-options.js
WASIX_SKIP_MISC_ENV_TESTS := \
  parallel/test-http2-tls-disconnect.js \
  parallel/test-http2-misbehaving-flow-control-paused.js
# Known in-process WASIX parity gaps (see 010_wasix_remaining_node_test_failures.md).
WASIX_SKIP_PARITY_TESTS := \
  client-proxy/test-http-proxy-request-connection-refused.mjs \
  client-proxy/test-https-proxy-request-connection-refused.mjs \
  sequential/test-http-econnrefused.js \
  sequential/test-tls-connect.js \
  parallel/test-dns-perf_hooks.js \
  parallel/test-dns-setserver-when-querying.js \
  parallel/test-http-writable-true-after-close.js \
  parallel/test-fastutf8stream-mode.js \
  parallel/test-tls-alert-handling.js \
  parallel/test-tls-error-stack.js \
  parallel/test-tls-hello-parser-failure.js \
  parallel/test-tls-junk-server.js
# WebCrypto tests do CPU-heavy keygen/sign/derive that is slow in wasm and races
# the 10s timeout under parallel WASIX load. Give the whole suite the scaled
# timeout rather than chasing them one flake at a time. (test-webcrypto-cryptokey-
# workers and test-webcrypto-derivebits-argon2 are omitted: they stay skipped in
# QUICKJS_SKIP_WORKER_TESTS / WASIX_SKIP_CRYPTO_UNSUPPORTED_TESTS respectively.)
WASIX_SLOW_WEBCRYPTO_TESTS := \
  parallel/test-crypto-webcrypto-aes-decrypt-tag-too-small.js \
  parallel/test-global-webcrypto-classes.js \
  parallel/test-global-webcrypto.js \
  parallel/test-webcrypto-constructors.js \
  parallel/test-webcrypto-derivebits-cfrg.js \
  parallel/test-webcrypto-derivebits-ecdh.js \
  parallel/test-webcrypto-derivebits-hkdf.js \
  parallel/test-webcrypto-derivebits.js \
  parallel/test-webcrypto-derivekey-cfrg.js \
  parallel/test-webcrypto-derivekey-ecdh.js \
  parallel/test-webcrypto-derivekey.js \
  parallel/test-webcrypto-digest.js \
  parallel/test-webcrypto-encap-decap-ml-kem.js \
  parallel/test-webcrypto-encrypt-decrypt-aes.js \
  parallel/test-webcrypto-encrypt-decrypt-chacha20-poly1305.js \
  parallel/test-webcrypto-encrypt-decrypt.js \
  parallel/test-webcrypto-encrypt-decrypt-rsa.js \
  parallel/test-webcrypto-export-import-cfrg.js \
  parallel/test-webcrypto-export-import-ec.js \
  parallel/test-webcrypto-export-import.js \
  parallel/test-webcrypto-export-import-ml-dsa.js \
  parallel/test-webcrypto-export-import-ml-kem.js \
  parallel/test-webcrypto-export-import-rsa.js \
  parallel/test-webcrypto-get-public-key.mjs \
  parallel/test-webcrypto-getRandomValues.js \
  parallel/test-webcrypto-internal-slots.mjs \
  parallel/test-webcrypto-keygen.js \
  parallel/test-webcrypto-keygen-kmac.js \
  parallel/test-webcrypto-random.js \
  parallel/test-webcrypto-sign-verify-ecdsa.js \
  parallel/test-webcrypto-sign-verify-eddsa.js \
  parallel/test-webcrypto-sign-verify-hmac.js \
  parallel/test-webcrypto-sign-verify.js \
  parallel/test-webcrypto-sign-verify-kmac.js \
  parallel/test-webcrypto-sign-verify-ml-dsa.js \
  parallel/test-webcrypto-sign-verify-rsa.js \
  parallel/test-webcrypto-supports.mjs \
  parallel/test-webcrypto-util.js \
  parallel/test-webcrypto-webidl.js \
  parallel/test-webcrypto-wrap-unwrap.js
# CI-only harness timeouts under parallel WASIX load (default harness timeout is 10s).
WASIX_SLOW_TESTS := \
  parallel/test-buffer-constants.js \
  parallel/test-crypto-oneshot-hash-xof.js \
  parallel/test-fastutf8stream-flush-sync.js \
  parallel/test-http2-respond-file-with-pipe.js \
  parallel/test-stringbytes-external.js \
  parallel/test-url-parse-invalid-input.js \
  $(WASIX_SLOW_WEBCRYPTO_TESTS)
WASIX_SLOW_TEST_TIMEOUT_SCALE ?= 12
WASIX_SKIP_ENV_TESTS ?= $(subst $(SPACE),$(COMMA),$(strip \
  $(WASIX_SKIP_UNIX_SOCKET_TESTS) \
  $(WASIX_SKIP_CLUSTER_FORK_TESTS) \
  $(WASIX_SKIP_SUBPROCESS_SHELL_TESTS) \
  $(WASIX_SKIP_OS_TESTS) \
  $(WASIX_SKIP_STACK_OVERFLOW_TESTS) \
  $(WASIX_SKIP_UDP_TESTS) \
  $(WASIX_SKIP_CRYPTO_UNSUPPORTED_TESTS) \
  $(WASIX_SKIP_TLS_SUBPROCESS_ENV_TESTS) \
  $(WASIX_SKIP_MISC_ENV_TESTS) \
  $(WASIX_SKIP_PARITY_TESTS)))

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
NAPI_V8_DIST_ROOT ?= $(CURDIR)/build-v8-napi/_v8_cache/$(NAPI_V8_PREBUILT_VERSION)/$(NAPI_V8_PLATFORM)
NAPI_V8_CMAKE_ARGS ?=
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

clean-napi-quickjs:
	rm -rf $(BUILD_EDGE_QUICKJS_CLI_DIR)

clean:
	find . -maxdepth 1 -type d -name 'build-*' -exec rm -rf {} +

build-napi:
	$(BUILD_ENV) cmake -S . -B $(BUILD_DIR) -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE) -DEDGE_DEFAULT_WASMER_PACKAGE=$(EDGE_WASMER_PACKAGE) -DEDGE_BUILD_NAPI_TESTS=ON $(NAPI_V8_CMAKE_ARGS) $(EXTRA_CMAKE_ARGS) $(CMAKE_ARGS)
	$(BUILD_ENV) cmake --build $(BUILD_DIR) -j$(JOBS)

build-napi-quickjs:
	$(BUILD_ENV) cmake -S . -B $(BUILD_EDGE_QUICKJS_CLI_DIR) -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE) -DEDGE_DEFAULT_WASMER_PACKAGE=$(EDGE_WASMER_PACKAGE) -DEDGE_NAPI_PROVIDER=quickjs -DEDGE_BUILD_NAPI_TESTS=ON $(EXTRA_CMAKE_ARGS) $(CMAKE_ARGS)
	$(BUILD_ENV) cmake --build $(BUILD_EDGE_QUICKJS_CLI_DIR) -j$(JOBS)

build-native-v8:
	$(MAKE) -C napi build-native-v8

build-native-quickjs:
	$(MAKE) -C napi build-native-quickjs

build:
	$(BUILD_ENV) cmake -S . -B $(BUILD_DIR) -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE) -DEDGE_DEFAULT_WASMER_PACKAGE=$(EDGE_WASMER_PACKAGE) -DEDGE_BUILD_NAPI_TESTS=OFF $(NAPI_V8_CMAKE_ARGS) $(EXTRA_CMAKE_ARGS) $(CMAKE_ARGS)
	$(BUILD_ENV) cmake --build $(BUILD_DIR) -j$(JOBS)

build-edge: build

build-edge-quickjs-cli:
	$(BUILD_ENV) cmake -S . -B $(BUILD_EDGE_QUICKJS_CLI_DIR) -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE) -DEDGE_DEFAULT_WASMER_PACKAGE=$(EDGE_WASMER_PACKAGE) -DEDGE_NAPI_PROVIDER=quickjs -DEDGE_BUILD_NAPI_TESTS=OFF $(EXTRA_CMAKE_ARGS) $(CMAKE_ARGS)
	$(BUILD_ENV) cmake --build $(BUILD_EDGE_QUICKJS_CLI_DIR) --target edge edgeenv -j$(JOBS)

build-wasix:
	./wasix/build-wasix.sh

build-quickjs-wasix:
	./quickjs-wasm/build.sh

$(QUICKJS_WASIX_WASM):
	./quickjs-wasm/build.sh

build-wasix-napi: build-wasix build-napi-wasmer-cli

build-wasix-napi-quickjs: build-quickjs-wasix

build-napi-wasmer-cli:
	cd $(NAPI_WASMER_DIR) && CARGO_TARGET_DIR="$(NAPI_WASMER_CARGO_TARGET_DIR)" ./cargo-standalone.sh build --features cli --bin napi_wasmer

test-wasix-napi: build-wasix-napi test-wasix-napi-cli

test-wasix-napi-quickjs: build-wasix-napi-quickjs
	$(MAKE) test-wasix-safe-mode WASIX_PACKAGE_DIR="$(CURDIR)/quickjs-wasm"

test-wasix-napi-cli: build-wasix build-napi-wasmer-cli
	@output="$$($(NAPI_WASMER_BINARY) $(WASIX_EDGEJS_WASM) -e "$(WASIX_NAPI_SMOKE_JS)")"; \
	printf '%s\n' "$$output"; \
	printf '%s\n' "$$output" | grep -Fx "hello world!"

test-wasix-safe-mode:
	python3 ./scripts/test-wasix-safe-mode.py --wasmer-bin "$(WASMER_BIN)" --package-dir "$(WASIX_PACKAGE_DIR)" $(WASIX_SAFE_MODE_ARGS)

$(EDGE_BINARY):
	$(MAKE) build

test: build test-only

test-only:
	EDGE_BYTECODE_CACHE=0 NODE_TEST_RUNNER=$(EDGE_BINARY) ./test/nodejs_test_harness --category=node:buffer,node:console,node:dgram,node:diagnostics_channel,node:dns,node:events,node:http,node:https,node:os,node:path,node:punycode,node:querystring,node:stream,node:string_decoder,node:tty,node:url,node:zlib,node:crypto,node:domain,node:http2,node:tls,node:sys \
	  --skip-tests=$(EDGE_NODE_TEST_SKIP_TESTS) \
	  -j $(TEST_JOBS)

test-quickjs-only:
	EDGE_BYTECODE_CACHE=0 NODE_TEST_RUNNER=$(BUILD_EDGE_QUICKJS_CLI_DIR)/edge ./test/nodejs_test_harness --category=node:buffer,node:console,node:dgram,node:diagnostics_channel,node:dns,node:events,node:http,node:https,node:os,node:path,node:punycode,node:querystring,node:stream,node:string_decoder,node:tty,node:url,node:zlib,node:crypto,node:domain,node:http2,node:tls,node:sys \
	  --skip-tests=$(QUICKJS_SKIP_TESTS) \
	  -j $(TEST_JOBS)

test-wasix-quickjs-only:
	WASIX_SLOW_TESTS="$(subst $(SPACE),$(COMMA),$(strip $(WASIX_SLOW_TESTS)))" \
	WASIX_SLOW_TEST_TIMEOUT_SCALE="$(WASIX_SLOW_TEST_TIMEOUT_SCALE)" \
	NODE_TEST_RUNNER=$(WASIX_QUICKJS_NODE_TEST_RUNNER) WASMER_BIN="$(WASMER_BIN)" EDGEJS_ROOT="$(CURDIR)" WASIX_EDGEJS_PACKAGE_DIR="$(CURDIR)/quickjs-wasm" ./test/nodejs_test_harness --category=node:buffer,node:console,node:dgram,node:diagnostics_channel,node:dns,node:events,node:http,node:https,node:os,node:path,node:punycode,node:querystring,node:stream,node:string_decoder,node:tty,node:url,node:zlib,node:crypto,node:domain,node:http2,node:tls,node:sys \
	  --skip-tests=$(QUICKJS_SKIP_TESTS),$(WASIX_SKIP_ENV_TESTS) \
	  -j $(TEST_JOBS)

# Node's own Intl / ICU locale tests. Run directly by path (not via a harness
# category) so we don't have to categorize them in the node-test submodule —
# the vendored test clone stays pristine. Excludes parallel/test-intl-
# v8BreakIterator (Intl absent in fresh vm realms) and parallel/test-icu-data-
# dir (--icu-data-dir handling); both are tracked in ECO-383.
QUICKJS_INTL_TESTS := \
  parallel/test-intl \
  parallel/test-icu-env \
  parallel/test-icu-minimum-version \
  parallel/test-icu-stringwidth \
  parallel/test-icu-transcode

test-quickjs-intl:
	@set -e; for t in $(QUICKJS_INTL_TESTS); do \
	  echo "[intl native] $$t"; \
	  EDGE_BYTECODE_CACHE=0 $(QUICKJS_EDGE_BINARY) "$(CURDIR)/test/$$t.js"; \
	done
	@echo "[intl native] all locale tests passed"

test-wasix-quickjs-intl:
	@command -v "$(WASMER_BIN)" >/dev/null 2>&1 || { \
		echo "error: $(WASMER_BIN) is required for test-wasix-quickjs-intl" >&2; exit 1; }
	@set -e; for t in $(QUICKJS_INTL_TESTS); do \
	  echo "[intl wasix] $$t"; \
	  WASMER_BIN="$(WASMER_BIN)" EDGEJS_ROOT="$(CURDIR)" WASIX_EDGEJS_PACKAGE_DIR="$(CURDIR)/quickjs-wasm" \
	    "$(WASIX_QUICKJS_NODE_TEST_RUNNER)" "$(CURDIR)/test/$$t.js"; \
	done
	@echo "[intl wasix] all locale tests passed"

test-bytecode-cache:
	EDGE_BIN=$(EDGE_BINARY) ./scripts/test-bytecode-cache.sh

test-bytecode-cache-quickjs:
	EDGE_BIN=$(BUILD_EDGE_QUICKJS_CLI_DIR)/edge ./scripts/test-bytecode-cache.sh

# Generate the shippable builtins (lib/) bytecode cache next to each binary
# (<binary>.builtins.v8b / .qjsb). The builtins cache is on by default, so a
# representative run populates it; ship the produced file alongside the binary
# so opt-in users (and read-only installs) get the builtin startup win from the
# first run. Re-run after rebuilding (the engine tag invalidates a stale cache).
PRECOMPILE_BUILTINS_DRIVER ?= ./benchmarks/precompile-builtins-driver.mjs
precompile-builtins:
	@for bin in $(EDGE_BINARY) $(BUILD_EDGE_QUICKJS_CLI_DIR)/edge; do \
		[ -x "$$bin" ] || continue; \
		rm -f "$$bin".builtins.*; \
		"$$bin" "$(PRECOMPILE_BUILTINS_DRIVER)" >/dev/null 2>&1 || true; \
		ls -la "$$bin".builtins.* 2>/dev/null || echo "no builtins cache produced for $$bin"; \
	done

clean-edge-quickjs-cli:
	rm -rf $(BUILD_EDGE_QUICKJS_CLI_DIR)

check-portability:
ifeq ($(UNAME_S),Darwin)
	@for bin in $(EDGE_BINARY) $(EDGEENV_BINARY); do \
		deps=$$(otool -L "$$bin" | tail -n +2 | awk '{print $$1}' | grep '^/' | grep -Ev '^(/System/Library/|/usr/lib/)' || true); \
		if [ -n "$$deps" ]; then \
			echo "error: $$bin links to non-system dylibs:" >&2; \
			echo "$$deps" >&2; \
			exit 1; \
		fi; \
		file "$$bin"; \
	done
endif

clean-dist:
	rm -rf $(DIST_DIR)
	rm -f $(ZIP_NAME)

dist: build dist-only

dist-only:
	rm -rf $(DIST_DIR)
	rm -f $(ZIP_NAME)
	mkdir -p $(DIST_BIN_DIR)
	if [ "$(BUILD_DIR)" = "build-wasix" ] || [ "$(BUILD_DIR)" = "$(BUILD_QUICKJS_WASIX_DIR)" ]; then \
		cp "$(BUILD_DIR)/edgejs.wasm" "$(DIST_BIN_DIR)/edgejs"; \
		cp wasmer.toml "$(DIST_DIR)/wasmer.toml"; \
		mkdir -p "$(DIST_DIR)/ssl-certs"; \
		cp "$(WASIX_SSL_CERTS_DIR)/cacert.pem" "$(DIST_DIR)/ssl-certs/cacert.pem"; \
		cp "$(WASIX_SSL_CERTS_DIR)/cert.pem" "$(DIST_DIR)/ssl-certs/cert.pem"; \
		cp -R "$(WASIX_SSL_CERTS_DIR)/certs" "$(DIST_DIR)/ssl-certs/certs"; \
		perl -0pi -e 's#^source = ".*"#source = "./bin/edgejs"#m' "$(DIST_DIR)/wasmer.toml"; \
	else \
		cp "$(EDGE_BINARY)" "$(DIST_BIN_DIR)/edge"; \
		cp "$(EDGEENV_BINARY)" "$(DIST_BIN_DIR)/edgeenv"; \
	fi
	cp -R bin-compat $(DIST_BIN_COMPAT_DIR)
	cp README.md $(DIST_DIR)/README.md
	if [ "$(UNAME_S)" = "Darwin" ] && [ "$(BUILD_DIR)" != "build-wasix" ] && [ "$(BUILD_DIR)" != "$(BUILD_QUICKJS_WASIX_DIR)" ]; then \
		for bin in $(DIST_BIN_DIR)/edge $(DIST_BIN_DIR)/edgeenv; do \
			deps=$$(otool -L "$$bin" | tail -n +2 | awk '{print $$1}' | grep '^/' | grep -Ev '^(/System/Library/|/usr/lib/)' || true); \
			if [ -n "$$deps" ]; then \
				echo "error: $$bin still links to non-system dylibs:" >&2; \
				echo "$$deps" >&2; \
				echo "Rebuild with 'make build' before packaging." >&2; \
				exit 1; \
			fi; \
		done; \
	fi
	if [ "$(BUILD_DIR)" = "build-wasix" ] || [ "$(BUILD_DIR)" = "$(BUILD_QUICKJS_WASIX_DIR)" ]; then \
		cd $(DIST_DIR) && zip -r ../$(ZIP_NAME) bin bin-compat README.md wasmer.toml ssl-certs; \
	else \
		cd $(DIST_DIR) && zip -r ../$(ZIP_NAME) bin bin-compat README.md; \
	fi

framework-test-run:
	@command -v "$(FRAMEWORK_TEST_ORCHESTRATOR)" >/dev/null 2>&1 || { \
		echo "error: $(FRAMEWORK_TEST_ORCHESTRATOR) is required to run framework-test" >&2; \
		exit 1; \
	}
	@SYMLINK_TARGET="$(SYMLINK_TARGET)" \
		EDGEJS_ROOT="$(CURDIR)" \
		FRAMEWORK_TEST_SKIP_SAFE="$(FRAMEWORK_TEST_SKIP_SAFE)" \
		FRAMEWORK_TEST_NODE_SKIP="$(FRAMEWORK_TEST_NODE_SKIP)" \
		FRAMEWORK_TEST_EDGE_SKIP="$(FRAMEWORK_TEST_EDGE_SKIP)" \
		FRAMEWORK_TEST_RUNNER_LABEL="$(FRAMEWORK_TEST_RUNNER_LABEL)" \
		"$(FRAMEWORK_TEST_ORCHESTRATOR)" "$(FRAMEWORK_TEST_SCRIPT)" test $(FRAMEWORK_TEST_SELECTOR)

framework-test: $(EDGE_BINARY)
	@"$(EDGE_BINARY)" "$(FRAMEWORK_TEST_SCRIPT)" test $(FRAMEWORK_TEST_SELECTOR)

# js-etherpad runs on both EdgeJS QuickJS Native and WASIX edge stages: the
# ICU-backed Intl surface (ECO-359) supplies Intl.ListFormat, native-function
# toString matches V8, and the example pre-bundles its client entrypoints at
# build time so no esbuild runs at runtime. (GC use-after-frees fixed in #101.)
framework-test-quickjs-native: $(QUICKJS_EDGE_BINARY)
	@SYMLINK_TARGET="$(abspath $(QUICKJS_EDGE_BINARY))" \
		FRAMEWORK_TEST_SKIP_SAFE=1 \
		FRAMEWORK_TEST_NODE_SKIP='js-docusaurus-staticsite,js-docusaurus2-staticsite' \
		FRAMEWORK_TEST_EDGE_SKIP='js-astro-ssr-standalone' \
		FRAMEWORK_TEST_RUNNER_LABEL='EdgeJS QuickJS Native' \
		$(MAKE) framework-test-run $(FRAMEWORK_TEST_SELECTOR)

framework-test-quickjs-wasix: $(QUICKJS_WASIX_WASM)
	@chmod +x "$(WASIX_FRAMEWORK_RUNNER)"
	@command -v "$(WASMER_BIN)" >/dev/null 2>&1 || { \
		echo "error: $(WASMER_BIN) is required for framework-test-quickjs-wasix" >&2; \
		exit 1; \
	}
	@SYMLINK_TARGET="$(abspath $(WASIX_FRAMEWORK_RUNNER))" \
		FRAMEWORK_TEST_SKIP_SAFE=1 \
		FRAMEWORK_TEST_NODE_SKIP='js-docusaurus-staticsite,js-docusaurus2-staticsite' \
		FRAMEWORK_TEST_EDGE_SKIP='js-astro-ssr-standalone' \
		FRAMEWORK_TEST_RUNNER_LABEL='EdgeJS QuickJS WASIX' \
		$(MAKE) framework-test-run $(FRAMEWORK_TEST_SELECTOR)

framework-test-reset:
	@if [ -x "$(EDGE_BINARY)" ]; then \
		"$(EDGE_BINARY)" "$(FRAMEWORK_TEST_SCRIPT)" reset $(FRAMEWORK_TEST_SELECTOR); \
	elif command -v node >/dev/null 2>&1; then \
		node "$(FRAMEWORK_TEST_SCRIPT)" reset $(FRAMEWORK_TEST_SELECTOR); \
	else \
		echo "error: $(EDGE_BINARY) is missing and no node fallback is available for framework-test-reset" >&2; \
		exit 1; \
	fi

standalone-build-test-run:
	@command -v "$(FRAMEWORK_TEST_ORCHESTRATOR)" >/dev/null 2>&1 || { \
		echo "error: $(FRAMEWORK_TEST_ORCHESTRATOR) is required to run standalone-build-test" >&2; \
		exit 1; \
	}
	@SYMLINK_TARGET="$(SYMLINK_TARGET)" \
		EDGEJS_ROOT="$(CURDIR)" \
		FRAMEWORK_TEST_SKIP_SAFE="$(FRAMEWORK_TEST_SKIP_SAFE)" \
		FRAMEWORK_TEST_NODE_SKIP="$(FRAMEWORK_TEST_NODE_SKIP)" \
		FRAMEWORK_TEST_EDGE_SKIP="$(FRAMEWORK_TEST_EDGE_SKIP)" \
		FRAMEWORK_TEST_RUNNER_LABEL="$(FRAMEWORK_TEST_RUNNER_LABEL)" \
		"$(FRAMEWORK_TEST_ORCHESTRATOR)" "$(STANDALONE_BUILD_TEST_SCRIPT)" test $(FRAMEWORK_TEST_SELECTOR)

standalone-build-test-quickjs-native: $(QUICKJS_EDGE_BINARY)
	@SYMLINK_TARGET="$(abspath $(QUICKJS_EDGE_BINARY))" \
		FRAMEWORK_TEST_SKIP_SAFE=1 \
		FRAMEWORK_TEST_EDGE_SKIP='js-astro-ssr-standalone' \
		FRAMEWORK_TEST_RUNNER_LABEL='EdgeJS QuickJS Native' \
		$(MAKE) standalone-build-test-run $(FRAMEWORK_TEST_SELECTOR)

standalone-build-test-quickjs-wasix: $(QUICKJS_WASIX_WASM)
	@chmod +x "$(WASIX_FRAMEWORK_RUNNER)"
	@command -v "$(WASMER_BIN)" >/dev/null 2>&1 || { \
		echo "error: $(WASMER_BIN) is required for standalone-build-test-quickjs-wasix" >&2; \
		exit 1; \
	}
	@SYMLINK_TARGET="$(abspath $(WASIX_FRAMEWORK_RUNNER))" \
		FRAMEWORK_TEST_SKIP_SAFE=1 \
		FRAMEWORK_TEST_EDGE_SKIP='js-astro-ssr-standalone' \
		FRAMEWORK_TEST_RUNNER_LABEL='EdgeJS QuickJS WASIX' \
		$(MAKE) standalone-build-test-run $(FRAMEWORK_TEST_SELECTOR)

js-%:
	@:
