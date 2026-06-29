# syntax=docker/dockerfile:1.10.0
#
# Cross-compile AgentOS darwin binaries via osxcross on a Linux runner. The
# base image carries osxcross, the macOS SDK, Node, and pnpm.
#
#   TARGET = aarch64-apple-darwin | x86_64-apple-darwin
#   CLANG  = aarch64-apple-darwin20.4 | x86_64-apple-darwin20.4
FROM ghcr.io/rivet-dev/rivet/builder-base-osxcross:0e33ceb98

ARG TARGET=aarch64-apple-darwin
ARG CLANG=aarch64-apple-darwin20.4
ARG TRIGGER=branch

ENV SDK=/root/osxcross/target/SDK/MacOSX11.3.sdk \
    RUSTC_WRAPPER=

WORKDIR /build
COPY . .

RUN rustup toolchain install stable --profile minimal && \
    rustup default stable && \
    rustup target add "$TARGET"

RUN corepack enable && \
    pnpm install --frozen-lockfile --filter='!@agentos/website'

# crates.io has no preview track: a secure-exec preview pin builds the crates
# from a clone at the pinned commit (../secure-exec == /secure-exec here);
# a release pin is a no-op and resolves crates from crates.io.
RUN node scripts/secure-exec-dep.mjs prepare-build

RUN tu=$(echo "$TARGET" | tr 'a-z-' 'A-Z_') && \
    tl=$(echo "$TARGET" | tr - _) && \
    export BINDGEN_EXTRA_CLANG_ARGS_${tl}="--sysroot=$SDK -isystem $SDK/usr/include" && \
    export CFLAGS_${tl}="-B/root/osxcross/target/bin" && \
    export CXXFLAGS_${tl}="-B/root/osxcross/target/bin" && \
    export CC_${tl}=${CLANG}-clang && \
    export CXX_${tl}=${CLANG}-clang++ && \
    export AR_${tl}=${CLANG}-ar && \
    export RANLIB_${tl}=${CLANG}-ranlib && \
    export CARGO_TARGET_${tu}_LINKER=${CLANG}-clang && \
    if [ "$TRIGGER" = "release" ]; then FLAG="--release"; PROF=release; else FLAG=""; PROF=debug; fi && \
    cargo build $FLAG -p agentos-sidecar -p agentos-actor-plugin --target "$TARGET" && \
    mkdir -p /artifacts && \
    cp "target/$TARGET/$PROF/agentos-sidecar" /artifacts/agentos-sidecar && \
    cp "target/$TARGET/$PROF/libagentos_actor_plugin.dylib" /artifacts/libagentos_actor_plugin.dylib

CMD ["ls", "-la", "/artifacts"]
