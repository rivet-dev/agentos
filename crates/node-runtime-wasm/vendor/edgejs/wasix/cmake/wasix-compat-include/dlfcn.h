#ifndef WASIX_COMPAT_DLFCN_H
#define WASIX_COMPAT_DLFCN_H

#include <errno.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifndef RTLD_LAZY
#define RTLD_LAZY 0x0001
#endif
#ifndef RTLD_NOW
#define RTLD_NOW 0x0002
#endif
#ifndef RTLD_GLOBAL
#define RTLD_GLOBAL 0x0100
#endif
#ifndef RTLD_LOCAL
#define RTLD_LOCAL 0x0000
#endif
#ifndef RTLD_DEFAULT
#define RTLD_DEFAULT ((void*) 0)
#endif

static inline void* dlopen(const char* filename, int flags) {
  (void) filename;
  (void) flags;
  errno = ENOSYS;
  return (void*) 0;
}

static inline int dlclose(void* handle) {
  (void) handle;
  errno = ENOSYS;
  return -1;
}

static inline void* dlsym(void* handle, const char* symbol) {
  (void) handle;
  (void) symbol;
  errno = ENOSYS;
  return (void*) 0;
}

static inline const char* dlerror(void) {
  return "dlfcn unsupported on WASIX";
}

#ifdef __cplusplus
}
#endif

#endif  // WASIX_COMPAT_DLFCN_H
