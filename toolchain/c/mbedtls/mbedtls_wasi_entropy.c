/*
 * Hardware entropy shim for the wasm32-wasip1 (WASI) mbedTLS build.
 *
 * The WASI build sets MBEDTLS_NO_PLATFORM_ENTROPY (no /dev/urandom fopen path)
 * and MBEDTLS_ENTROPY_HARDWARE_ALT, which routes the entropy module through
 * this mbedtls_hardware_poll() implementation. We satisfy it with getentropy(),
 * the WASI random primitive (backed by the kernel's random device layer) that
 * the repo's host-side wasi_tls.c already uses. getentropy() fills at most 256
 * bytes per call, so we loop for larger requests.
 */

#include <stddef.h>
#include <time.h>   /* clock_gettime(), CLOCK_MONOTONIC */
#include <unistd.h> /* getentropy() — declared by wasi-libc <unistd.h> */

#include "mbedtls/build_info.h"
#include "mbedtls/platform_util.h" /* mbedtls_ms_time_t */

/*
 * WASI mbedtls_ms_time() over clock_gettime(CLOCK_MONOTONIC). Enabled by
 * MBEDTLS_PLATFORM_MS_TIME_ALT in the WASI user config (see wasi_user_config.h),
 * which suppresses mbedTLS's own platform-detected definition.
 */
#if defined(MBEDTLS_PLATFORM_MS_TIME_ALT)
mbedtls_ms_time_t mbedtls_ms_time(void)
{
    struct timespec tv;
    if (clock_gettime(CLOCK_MONOTONIC, &tv) != 0) {
        return (mbedtls_ms_time_t) time(NULL) * 1000;
    }
    return (mbedtls_ms_time_t) tv.tv_sec * 1000 +
           (mbedtls_ms_time_t) (tv.tv_nsec / 1000000);
}
#endif /* MBEDTLS_PLATFORM_MS_TIME_ALT */

int mbedtls_hardware_poll(void *data, unsigned char *output, size_t len,
                          size_t *olen)
{
    (void)data;

    size_t filled = 0;
    while (filled < len) {
        size_t chunk = len - filled;
        if (chunk > 256) {
            chunk = 256; /* getentropy() rejects requests larger than 256. */
        }
        if (getentropy(output + filled, chunk) != 0) {
            if (olen != NULL) {
                *olen = filled;
            }
            return -1; /* MBEDTLS_ERR_ENTROPY_SOURCE_FAILED at the call site. */
        }
        filled += chunk;
    }

    if (olen != NULL) {
        *olen = len;
    }
    return 0;
}
