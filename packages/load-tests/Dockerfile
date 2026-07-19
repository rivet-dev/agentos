# syntax=docker/dockerfile:1.7

# Rust >= 1.91.1 is required by the aws-sdk / smithy dependency tree; pin to the
# workspace's toolchain (matches the canonical host: cargo 1.97).
FROM rust:1.97-bookworm AS build

ARG NODE_VERSION=24.4.1
ARG TARGETARCH

RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates clang cmake curl libssl-dev pkg-config python3 xz-utils \
	&& rm -rf /var/lib/apt/lists/*

RUN case "$TARGETARCH" in \
		amd64) node_arch=x64 ;; \
		arm64) node_arch=arm64 ;; \
		*) echo "unsupported TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
	esac \
	&& curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
		| tar -xJ --strip-components=1 -C /usr/local

RUN corepack enable && corepack prepare pnpm@10.13.1 --activate

WORKDIR /app
COPY . .

# Full install so every package in the build chain has its dev toolchain (tsc,
# tsup). The chain is heavier than the cargo build only in disk, not wall-clock.
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
	pnpm install --frozen-lockfile
# The exact-workspace release sidecar the VMs run against. Cargo's registry and
# target dir are cache-mounted so editing TypeScript (which busts `COPY . .`)
# does not force a full recompile. The target dir is a cache mount and is not
# part of the image layer, so the binary is copied to a persisted path.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/app/target \
	cargo build --release -p agentos-sidecar \
	&& mkdir -p /app/release-bin \
	&& cp /app/target/release/agentos-sidecar /app/release-bin/agentos-sidecar
# Build only the TS packages the load runner needs, in dependency order, without
# the WASM toolchain (see packages/load-tests/build-deps.sh for why that is
# sound for node-only, defaultSoftware:false workloads).
RUN bash packages/load-tests/build-deps.sh

FROM node:24-bookworm-slim

RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates tini \
	&& rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /app /app

ENV AGENTOS_SIDECAR_BIN=/app/release-bin/agentos-sidecar
ENV LOAD_TEST_ARTIFACT_DIR=/artifacts
ENV RIVET_PORT=3000

EXPOSE 3000
ENTRYPOINT ["/usr/bin/tini", "--", "node", "--expose-gc", "packages/load-tests/dist/cli.js"]
CMD ["compute-server"]
