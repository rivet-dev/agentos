/*
 * mbedTLS user-config overrides for the wasm32-wasip1 (WASI) build.
 *
 * This file is included via -DMBEDTLS_USER_CONFIG_FILE after mbedTLS's own
 * default `mbedtls_config.h`, so it only needs to describe the deltas required
 * to cross-compile cleanly against the wasi-sdk sysroot. The default 3.6 config
 * already enables TLS 1.2 + TLS 1.3, X.509, PK, PSA crypto, and MBEDTLS_FS_IO
 * (needed for curl/wget's mbedtls_x509_crt_parse_file on /etc/ssl/certs).
 *
 * Deltas:
 *   - Single-threaded: MBEDTLS_THREADING_C stays off (default) — no pthreads.
 *   - Entropy: WASI has no /dev/urandom fopen path, so disable platform entropy
 *     and route the entropy module through mbedtls_hardware_poll(), which we
 *     implement over getentropy() (see mbedtls_wasi_entropy.c). getentropy() is
 *     the same primitive the repo's host-side wasi_tls.c already relies on.
 *   - Networking: MBEDTLS_NET_C pulls in BSD <sys/socket.h> select()/socket()
 *     that wasi-libc does not fully provide. curl/wget own the transport and
 *     only need the TLS record + X.509 layers, so drop mbedTLS's own sockets.
 *   - Timing: MBEDTLS_TIMING_C uses gettimeofday()/select() hardware timers that
 *     are unnecessary here (no DTLS retransmission timers in the curl path).
 */

#ifndef MBEDTLS_WASI_USER_CONFIG_H
#define MBEDTLS_WASI_USER_CONFIG_H

/* Entropy: replace the POSIX /dev/urandom source with a getentropy() poll. */
#undef MBEDTLS_PLATFORM_ENTROPY
#define MBEDTLS_NO_PLATFORM_ENTROPY
#define MBEDTLS_ENTROPY_HARDWARE_ALT

/*
 * mbedtls_ms_time(): mbedTLS's built-in POSIX branch only compiles when it can
 * see _POSIX_VERSION, but it guards the <unistd.h> include behind unix/__unix
 * macros that clang does not define for wasm32, so the platform detection falls
 * through to `#error "No mbedtls_ms_time available"`. Provide it ourselves over
 * clock_gettime(CLOCK_MONOTONIC) (available in wasi-libc) via the ALT hook.
 */
#define MBEDTLS_PLATFORM_MS_TIME_ALT

/* Drop mbedTLS's own BSD-sockets and hardware-timing modules for WASI. */
#undef MBEDTLS_NET_C
#undef MBEDTLS_TIMING_C

#endif /* MBEDTLS_WASI_USER_CONFIG_H */
