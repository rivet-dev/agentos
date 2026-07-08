/**
 * Fix for wasi-libc's swprintf errno handling.
 *
 * musl's swprintf returns -1 when the buffer is too small but doesn't
 * set errno to EOVERFLOW as POSIX requires. This fix wraps vswprintf
 * to set errno correctly.
 *
 * Installed into the patched sysroot so ALL WASM programs get correct
 * swprintf behavior, not just test binaries.
 */

#include <wchar.h>
#include <errno.h>
#include <stdarg.h>

int swprintf(wchar_t *restrict s, size_t n,
             const wchar_t *restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = vswprintf(s, n, fmt, ap);
    va_end(ap);
    if (ret < 0) {
        errno = EOVERFLOW;
    }
    return ret;
}
