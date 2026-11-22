# syntax=docker/dockerfile:1.10.0
#
# Cross-compile AgentOS darwin binaries via osxcross on a Linux runner. The
# base image carries osxcross, the macOS SDK, Node, and pnpm.
#
#   TARGET = aarch64-apple-darwin | x86_64-apple-darwin
#   CLANG  = aarch64-apple-darwin20.4 | x86_64-apple-darwin20.4
FROM scratch AS agentos-v8-inputs
COPY scripts/ci/v8/archives.json /archives.json

FROM ghcr.io/rivet-dev/rivet/builder-base-osxcross:0e33ceb98

ARG TARGET=aarch64-apple-darwin
ARG CLANG=aarch64-apple-darwin20.4
ARG BUILD_PROFILE=debug
ARG CACHE_PLATFORM=darwin-arm64
ARG RUST_TOOLCHAIN=1.95.0
ARG DARWIN_LLVM_VERSION=21

ENV SDK=/root/osxcross/target/SDK/MacOSX14.5.sdk \
    SDKROOT=/root/osxcross/target/SDK/MacOSX14.5.sdk \
    OSXCROSS_SDK=MacOSX14.5.sdk \
    RUSTC_WRAPPER=sccache \
    SCCACHE_DIR=/root/.cache/sccache \
    SCCACHE_IDLE_TIMEOUT=0

# V8's public headers and agentOS C++ shims require a C++23 compiler.
# Keep the osxcross SDK/linker wrappers, but select the modern host compiler.
RUN set -eux; \
    . /etc/os-release; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl gnupg; \
    curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
      | gpg --dearmor > /etc/apt/trusted.gpg.d/llvm-snapshot.gpg; \
    echo "deb https://apt.llvm.org/${VERSION_CODENAME}/ llvm-toolchain-${VERSION_CODENAME}-${DARWIN_LLVM_VERSION} main" \
      > /etc/apt/sources.list.d/agentos-llvm.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends "clang-${DARWIN_LLVM_VERSION}"; \
    curl -fsSL https://github.com/joseluisq/macosx-sdks/releases/download/14.5/MacOSX14.5.sdk.tar.xz -o /tmp/macos-sdk.tar.xz; \
    echo "6e146275d19f027faa2e8354da5e0267513abf013b8f16ad65a231653a2b1c5d  /tmp/macos-sdk.tar.xz" | sha256sum -c -; \
    tar -xJf /tmp/macos-sdk.tar.xz -C /root/osxcross/target/SDK; \
    rm /tmp/macos-sdk.tar.xz; \
    rm -rf /var/lib/apt/lists/*

ENV PATH="/root/osxcross/target/bin:/usr/lib/llvm-${DARWIN_LLVM_VERSION}/bin:${PATH}"

WORKDIR /build
COPY scripts/ci/v8 /build/scripts/ci/v8
RUN --mount=type=bind,from=agentos-v8-inputs,target=/opt/engine-inputs \
    python3 scripts/ci/v8/engine.py install "${TARGET}" /opt/agentos-v8 --build-context /opt/engine-inputs --cargo-config /opt/agentos-v8.toml --shell-env /opt/agentos-v8.sh

COPY . .

RUN rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal && \
    rustup default "$RUST_TOOLCHAIN" && \
    rustup target add "$TARGET"

RUN --mount=type=cache,id=pnpm-store-agentos-darwin,target=/root/.local/share/pnpm/store,sharing=locked \
    corepack enable && \
    pnpm config set store-dir /root/.local/share/pnpm/store && \
    pnpm install --frozen-lockfile

RUN --mount=type=cache,id=cargo-registry-agentos-darwin,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git-agentos-darwin,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-agentos-${CACHE_PLATFORM},target=/build/target,sharing=locked \
    --mount=type=cache,id=sccache-agentos-${CACHE_PLATFORM},target=/root/.cache/sccache,sharing=locked \
    if ! command -v sccache >/dev/null 2>&1; then \
      unset RUSTC_WRAPPER; \
    elif ! (sccache --start-server 2>/tmp/sccache-start.err && sccache --show-stats >/dev/null 2>&1); then \
      echo "[sccache] unavailable, disabling:"; cat /tmp/sccache-start.err 2>/dev/null || true; \
      sccache --stop-server >/dev/null 2>&1 || true; \
      unset RUSTC_WRAPPER SCCACHE_DIR; \
    else \
      echo "[sccache] enabled via local BuildKit cache"; \
    fi && \
    tu=$(echo "$TARGET" | tr 'a-z-' 'A-Z_') && \
    tl=$(echo "$TARGET" | tr - _) && \
    export BINDGEN_EXTRA_CLANG_ARGS_${tl}="--sysroot=$SDK -isystem $SDK/usr/include" && \
    export CFLAGS_${tl}="-B/root/osxcross/target/bin" && \
    export CXXFLAGS_${tl}="-B/root/osxcross/target/bin" && \
    export CC_${tl}=${CLANG}-clang && \
    export CXX_${tl}=${CLANG}-clang++ && \
    export AR_${tl}=${CLANG}-ar && \
    export RANLIB_${tl}=${CLANG}-ranlib && \
    export CARGO_TARGET_${tu}_LINKER=${CLANG}-clang && \
    if [ "$BUILD_PROFILE" = "release" ]; then FLAG="--release"; PROF=release; else FLAG=""; PROF=debug; fi && \
    . /opt/agentos-v8.sh && \
    cargo --config /opt/agentos-v8.toml build $FLAG -p agentos-sidecar --target "$TARGET" && \
    mkdir -p /artifacts && \
    cp "target/$TARGET/$PROF/agentos-sidecar" /artifacts/agentos-sidecar && \
    (sccache --show-stats 2>/dev/null || true)

CMD ["ls", "-la", "/artifacts"]
