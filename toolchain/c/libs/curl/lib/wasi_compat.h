/*
 * wasi_compat.h - Compatibility shims for WASI wasip1 target
 *
 * The WASI wasip1 sysroot guards many POSIX socket declarations behind
 * __wasilibc_unmodified_upstream or __wasilibc_use_wasip2. Our patched
 * sysroot provides the implementations (via host_net), but the headers
 * don't declare them for wasip1. We provide the missing declarations here.
 */

#ifndef HEADER_WASI_COMPAT_H
#define HEADER_WASI_COMPAT_H

#ifdef __wasi__

#include <sys/socket.h>

/* Function declarations hidden behind wasip2 guard in sys/socket.h */
int getsockname(int, struct sockaddr *__restrict, socklen_t *__restrict);
int getpeername(int, struct sockaddr *__restrict, socklen_t *__restrict);

/* struct sockaddr_un is behind __wasilibc_unmodified_upstream */
#ifndef _SUN_PATH_DEFINED
#define _SUN_PATH_DEFINED
struct sockaddr_un {
    sa_family_t sun_family;
    char sun_path[108];
};
#endif

#endif /* __wasi__ */

#endif /* HEADER_WASI_COMPAT_H */
