#include <dlfcn.h>
#include <errno.h>
#include <stddef.h>

/*
 * POSIX does not standardize dlopen; Linux specifies it in dlopen(3):
 * https://man7.org/linux/man-pages/man3/dlopen.3.html
 *
 * Arbitrary native host libraries cannot enter the V8 isolate. The separate
 * validated WASM side-module loader owns Node-API addon loading. Until a call
 * is routed there, libc reports a hard loader error rather than importing an
 * unrestricted host dynamic-loader capability or pretending success.
 */
static _Thread_local const char *loader_error;

void *dlopen(const char *path, int mode) {
    (void)path;
    (void)mode;
    loader_error = "dynamic host libraries are unavailable; use a validated WASM addon";
    errno = ENOSYS;
    return NULL;
}

void *dlsym(void *handle, const char *symbol) {
    (void)handle;
    (void)symbol;
    loader_error = "dynamic symbol lookup is unavailable for host libraries";
    errno = ENOSYS;
    return NULL;
}

int dlclose(void *handle) {
    (void)handle;
    loader_error = "dynamic host library handle is invalid";
    errno = EINVAL;
    return -1;
}

char *dlerror(void) {
    const char *message = loader_error;
    loader_error = NULL;
    return (char *)message;
}
