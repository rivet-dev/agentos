WASM_TARGET := wasm32-wasip1
RELEASE_DIR := target/$(WASM_TARGET)/release

# Standalone binary output directory (configurable)
COMMANDS_DIR ?= $(RELEASE_DIR)/commands

# Detect patched std: if std-patches/ has .patch files and patch-std.sh exists, use patched sysroot
PATCHES := $(wildcard std-patches/*.patch)
PATCH_SCRIPT := scripts/patch-std.sh

# Resolve WASI sysroot for CRT/libc needed by -Z build-std linking
# Uses rustc without +toolchain override so rust-toolchain.toml is respected
WASI_SYSROOT_SELF_CONTAINED := $(shell rustc --print sysroot)/lib/rustlib/$(WASM_TARGET)/lib/self-contained

# Discover command binary names from colocated package crates plus toolchain
# helper crates. Keep known slow/heavy commands out of the default software
# gate; they can still be built explicitly
# with `make cmd/<name>`.
SOFTWARE_COMMAND_CRATES := $(wildcard ../software/*/native/crates/cmd-*)
TOOLCHAIN_COMMAND_CRATES := $(wildcard crates/commands/*)
SKIPPED_BULK_COMMANDS := _stubs git codex codex-exec
COMMAND_NAMES := $(filter-out $(SKIPPED_BULK_COMMANDS),$(patsubst cmd-%,%,$(notdir $(SOFTWARE_COMMAND_CRATES))) $(notdir $(TOOLCHAIN_COMMAND_CRATES)))
COMMAND_PACKAGES := $(addprefix -p cmd-,$(COMMAND_NAMES))

# Alias symlinks: link_name:target_binary
ALIAS_SYMLINKS := \
	egrep:grep fgrep:grep \
	gunzip:gzip zcat:gzip \
	bash:sh \
	dir:ls vdir:ls \
	more:cat \
	[:test

# Stub commands: each gets a symlink to _stubs
STUB_COMMANDS := \
	chcon runcon \
	chgrp chown chroot \
	df \
	groups id \
	hostname hostid \
	install \
	kill \
	mkfifo mknod \
	pinky who users uptime \
	stty sync tty

.PHONY: all commands wasm wasm-opt-check host test clean patch-std patch-check vendor patch-vendor vendor-patch-check size-report install c-wasm codex

all: wasm

# Build the fast command set that lands in COMMANDS_DIR: Rust commands except
# known heavy/external ones, then the opted-in fast C commands. Heavy commands
# (git, duckdb, vim), generated helpers (_stubs), and the external codex build
# are intentionally excluded from the default software-build gate.
commands: wasm
	$(MAKE) -C c programs install

# Build the real wasm32-wasip1 `codex-exec` agent engine from the codex fork
# (branch wasi-port-codex-core). The fork ships a committed, idempotent, self-prepping build script
# (it prepares the rustup sysroot for -Z build-std and restores it on exit — no manual stash), so
# this folds codex into the toolchain: `make -C toolchain codex` produces and installs
# software/codex/wasm/{codex-exec,codex}. Override CODEX_REPO if the fork is not at the
# default sibling path. See the fork's docs/wasi-build.md for the toolchain constraints, and the
# remaining work to source the CRT from wasi-sdk so the prebuilt rust target need not be installed.
AGENTOS_ROOT := $(abspath $(CURDIR)/..)
CODEX_REPO ?= $(abspath $(AGENTOS_ROOT)/../codex-rs/codex-rs)
codex:
	@if [ -x "$(CODEX_REPO)/scripts/build-wasi-codex-exec.sh" ]; then \
		AGENTOS_DIR="$(AGENTOS_ROOT)" "$(CODEX_REPO)/scripts/build-wasi-codex-exec.sh"; \
	else \
		echo "codex: fork build script not found at $(CODEX_REPO)/scripts/build-wasi-codex-exec.sh — skipping."; \
		echo "       Set CODEX_REPO=/path/to/codex-rs/codex-rs (the wasi-port-codex-core checkout) to build it."; \
	fi

# Strict variant (fails if the fork is absent) for environments that require the codex artifact.
.PHONY: codex-required
codex-required:
	@test -x "$(CODEX_REPO)/scripts/build-wasi-codex-exec.sh" || { \
		echo "ERROR: codex fork build script not found at $(CODEX_REPO); set CODEX_REPO"; exit 1; }
	@$(MAKE) codex

# Apply std patches if they exist
patch-std:
ifneq ($(PATCHES),)
	@if [ -x "$(PATCH_SCRIPT)" ]; then \
		echo "Applying std patches..."; \
		./$(PATCH_SCRIPT); \
	else \
		echo "Warning: patches found but $(PATCH_SCRIPT) not executable or missing"; \
	fi
else
	@echo "No patches found, using stock std"
endif

# Resolve Rust std source for vendoring build-std deps
RUST_STD_SRC := $(shell rustc --print sysroot)/lib/rustlib/src/rust

# Vendor dependencies from crates.io into vendor/ (includes std deps for -Z build-std)
vendor:
	@if [ ! -d vendor ]; then \
		echo "Vendoring dependencies (including std library deps)..."; \
		mkdir -p .cargo; \
		cargo vendor \
			--sync "$(RUST_STD_SRC)/library/std/Cargo.toml" \
			--sync "$(RUST_STD_SRC)/library/test/Cargo.toml" \
			vendor > .cargo/config.toml; \
	else \
		echo "vendor/ exists — skipping (run 'make clean-vendor' to re-vendor)"; \
	fi
	@# Source-replacement config so cargo builds from the patched vendor/ tree
	@# instead of the crates.io registry cache. This file is gitignored, so a
	@# fresh checkout (CI) has none; without it every std-patches/crates/*.patch
	@# wasi fix is silently ignored and crates like uu_tac fail to compile for
	@# wasm32-wasip1. `cargo vendor` prints the exact config (including the
	@# synced std-dep sources) on stdout, so capture it above.
	@test -s .cargo/config.toml || { \
		echo "ERROR: .cargo/config.toml missing or empty after vendor"; \
		echo "Run 'make clean-vendor && make vendor' to regenerate."; \
		exit 1; \
	}

# Apply crate-level patches to vendored sources
patch-vendor: vendor
	@if [ -x scripts/patch-vendor.sh ]; then \
		./scripts/patch-vendor.sh; \
	else \
		echo "Warning: scripts/patch-vendor.sh not found or not executable"; \
	fi

# Dry-run verification of crate patches
vendor-patch-check: vendor
	@if [ -x scripts/patch-vendor.sh ]; then \
		./scripts/patch-vendor.sh --check; \
	else \
		echo "Warning: scripts/patch-vendor.sh not found or not executable"; \
	fi

# Ensure wasm-opt is installed (needed for post-build optimization)
wasm-opt-check:
	@if ! command -v wasm-opt >/dev/null 2>&1; then \
		echo "wasm-opt not found — installing via cargo..."; \
		cargo install wasm-opt; \
	else \
		echo "wasm-opt found: $$(wasm-opt --version)"; \
	fi

# Build all standalone command binaries, optimize, strip .wasm extension, create symlinks
wasm: vendor patch-vendor patch-std wasm-opt-check
	# --cfg tokio_unstable lifts tokio's wasm `compile_error!` so the patched
	# tokio process/net modules (std-patches/crates/tokio) compile on wasm32-wasip1.
	# Needed by codex (tokio::process/net). Harmless for commands not using tokio.
	RUSTFLAGS="-C link-arg=-L$(WASI_SYSROOT_SELF_CONTAINED) --cfg tokio_unstable" \
	cargo build --target $(WASM_TARGET) \
		-Z build-std=std,panic_abort \
		--release \
		$(COMMAND_PACKAGES)
	@mkdir -p $(COMMANDS_DIR)
	@TOTAL=0; SKIPPED=0; \
	HAS_OPT=$$(command -v wasm-opt >/dev/null 2>&1 && echo 1 || echo 0); \
	for cmd in $(COMMAND_NAMES); do \
		wasm="$(RELEASE_DIR)/$$cmd.wasm"; \
		if [ -f "$$wasm" ]; then \
			TOTAL=$$((TOTAL + 1)); \
			if [ "$$HAS_OPT" = "1" ]; then \
				wasm-opt -O3 --strip-debug --all-features "$$wasm" -o "$(COMMANDS_DIR)/$$cmd"; \
			else \
				cp "$$wasm" "$(COMMANDS_DIR)/$$cmd"; \
			fi; \
		else \
			SKIPPED=$$((SKIPPED + 1)); \
			echo "Warning: $$wasm not found — skipping $$cmd"; \
		fi; \
	done; \
	echo ""; \
	echo "=== Standalone Binary Build Report ==="; \
	echo "Binaries: $$TOTAL built"; \
	if [ $$SKIPPED -gt 0 ]; then \
		echo "Skipped:  $$SKIPPED (binary not found)"; \
	fi; \
	if [ "$$HAS_OPT" = "1" ]; then \
		echo "Optimized: wasm-opt -O3 --strip-debug"; \
	else \
		echo "Optimized: none (wasm-opt not found)"; \
	fi
	@# Create alias symlinks (fall back to copy on platforms without symlink support)
	@cd $(COMMANDS_DIR) && \
	for pair in $(ALIAS_SYMLINKS); do \
		link=$${pair%%:*}; target=$${pair##*:}; \
		if [ -f "$$target" ]; then \
			ln -sf "$$target" "$$link" 2>/dev/null || cp "$$target" "$$link"; \
		fi; \
	done
	@# Create stub command symlinks
	@cd $(COMMANDS_DIR) && \
	for stub in $(STUB_COMMANDS); do \
		if [ -f _stubs ]; then \
			ln -sf _stubs "$$stub" 2>/dev/null || cp _stubs "$$stub"; \
		fi; \
	done
	@TOTAL_FILES=$$(ls -1 $(COMMANDS_DIR) 2>/dev/null | wc -l); \
	echo "Total:    $$TOTAL_FILES entries in $(COMMANDS_DIR)"; \
	echo "=== Build complete ==="

# ---------------------------------------------------------------------------
# Uniform per-binary entry point: `make cmd/<name>` builds ONE command binary
# into $(COMMANDS_DIR), dispatching to the toolchain that owns it:
#   Rust      software/*/native/crates/cmd-<name> or crates/commands/<name>
#             (cargo package cmd-<name>, via wasm-cmd)
#   C         the C opt-in set below  (toolchain/c, patched sysroot)
#   codex     the codex fork build    (codex, codex-exec)
# `just toolchain-cmd <name>` calls this. `make wasm` builds the fast
# Rust set; `make -C c programs install` builds/installs the fast C set.
C_COMMANDS := zip unzip envsubst sqlite3 curl wget duckdb vim http_get

.PHONY: cmd/%
cmd/%:
	@name="$*"; \
	crate_dir=$$(find ../software -path "*/native/crates/cmd-$$name" -type d -print -quit); \
	if [ -n "$$crate_dir" ] || [ -d "crates/commands/$$name" ]; then \
		$(MAKE) wasm-cmd CMD=$$name; \
	elif echo " $(C_COMMANDS) " | grep -q " $$name "; then \
		src=$$name; [ "$$name" = "sqlite3" ] && src=sqlite3_cli; \
		$(MAKE) -C c sysroot "build/$$src" && $(MAKE) -C c install COMMANDS="$$name"; \
	elif [ "$$name" = "codex" ] || [ "$$name" = "codex-exec" ]; then \
		$(MAKE) codex-required; \
	else \
		echo "ERROR: unknown command '$$name' (no software/*/native/crates/cmd-$$name or crates/commands/$$name; not in C_COMMANDS or codex)"; \
		exit 1; \
	fi

# Build ONE command binary (cargo package cmd-$(CMD) -> $(COMMANDS_DIR)/$(CMD)).
# Usage: make wasm-cmd CMD=sh
.PHONY: wasm-cmd
wasm-cmd: vendor patch-vendor patch-std wasm-opt-check
	@test -n "$(CMD)" || { echo "ERROR: pass CMD=<name> (a dir under crates/commands/)"; exit 1; }
	@crate_dir=$$(find ../software -path "*/native/crates/cmd-$(CMD)" -type d -print -quit); \
	test -n "$$crate_dir" || test -d "crates/commands/$(CMD)" || { \
		echo "ERROR: no software/*/native/crates/cmd-$(CMD) or crates/commands/$(CMD)"; exit 1; }
	RUSTFLAGS="-C link-arg=-L$(WASI_SYSROOT_SELF_CONTAINED) --cfg tokio_unstable" \
	cargo build --target $(WASM_TARGET) \
		-Z build-std=std,panic_abort \
		--release \
		-p cmd-$(CMD)
	@mkdir -p $(COMMANDS_DIR)
	@if command -v wasm-opt >/dev/null 2>&1; then \
		wasm-opt -O3 --strip-debug --all-features "$(RELEASE_DIR)/$(CMD).wasm" -o "$(COMMANDS_DIR)/$(CMD)"; \
	else \
		cp "$(RELEASE_DIR)/$(CMD).wasm" "$(COMMANDS_DIR)/$(CMD)"; \
	fi
	@echo "Built $(COMMANDS_DIR)/$(CMD)"

# Show per-command binary sizes in COMMANDS_DIR
size-report:
	@if [ ! -d "$(COMMANDS_DIR)" ]; then \
		echo "Error: $(COMMANDS_DIR) not found. Run 'make wasm' first."; \
		exit 1; \
	fi
	@echo "=== Standalone Binary Size Report ==="
	@echo ""
	@printf "%-10s  %s\n" "SIZE" "COMMAND"
	@printf "%-10s  %s\n" "----" "-------"
	@ls -lhS $(COMMANDS_DIR)/* 2>/dev/null | \
		grep -v "^total" | \
		while read line; do \
			size=$$(echo "$$line" | awk '{print $$5}'); \
			name=$$(echo "$$line" | awk '{print $$NF}'); \
			base=$$(basename "$$name"); \
			link=""; \
			if [ -L "$$name" ]; then \
				target=$$(readlink "$$name"); \
				link=" -> $$target"; \
			fi; \
			printf "%-10s  %s%s\n" "$$size" "$$base" "$$link"; \
		done
	@echo ""
	@TOTAL_SIZE=$$(du -sh $(COMMANDS_DIR) 2>/dev/null | awk '{print $$1}'); \
	TOTAL_FILES=$$(ls -1 $(COMMANDS_DIR) 2>/dev/null | wc -l); \
	REAL_FILES=$$(find $(COMMANDS_DIR) -maxdepth 1 -type f 2>/dev/null | wc -l); \
	SYMLINKS=$$(find $(COMMANDS_DIR) -maxdepth 1 -type l 2>/dev/null | wc -l); \
	echo "Total:     $$TOTAL_SIZE ($$TOTAL_FILES entries: $$REAL_FILES binaries, $$SYMLINKS symlinks)"
	@echo ""
	@echo "=== Per-Crate Size Contribution (rlib, top 30) ==="
	@echo "Note: rlib sizes are pre-LTO; actual contribution after LTO/DCE may differ."
	@echo ""
	@printf "%-10s  %s\n" "SIZE" "CRATE"
	@printf "%-10s  %s\n" "----" "-----"
	@ls -lhS $(RELEASE_DIR)/deps/*.rlib 2>/dev/null | \
		awk '{split($$NF, a, "/"); name=a[length(a)]; gsub(/^lib/, "", name); gsub(/-[0-9a-f]+\.rlib$$/, "", name); printf "%-10s  %s\n", $$5, name}' | \
		awk '!seen[$$2]++' | \
		head -30
	@echo ""
	@echo "=== Dependency Count ==="
	@TOTAL=$$(ls $(RELEASE_DIR)/deps/*.rlib 2>/dev/null | \
		sed 's/.*\/lib//' | sed 's/-[0-9a-f]*\.rlib//' | sort -u | wc -l); \
	echo "Unique crates: $$TOTAL"

# Install standalone binaries to a custom directory

# Build C programs to WASM (downloads wasi-sdk if needed)


test:
	cargo test --workspace

clean:
	cargo clean
	rm -rf host/dist

clean-vendor:
	rm -rf vendor .cargo/config.toml

patch-check:
ifneq ($(PATCHES),)
	@if [ -x "$(PATCH_SCRIPT)" ]; then \
		./$(PATCH_SCRIPT) --check; \
	else \
		echo "Warning: $(PATCH_SCRIPT) not found or not executable"; \
	fi
else
	@echo "No patches found"
endif
