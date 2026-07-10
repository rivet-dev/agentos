/*
 * wasi_stubs.c - Stub implementations for POSIX functions missing from
 * the WASI wasip1 sysroot that libcurl needs.
 *
 * These stubs provide minimal implementations that satisfy the linker
 * without requiring full kernel support. The functions return plausible
 * defaults for a sandboxed environment.
 */

#ifdef __wasi__

#include <sys/socket.h>
#include <netinet/in.h>
#include <string.h>
#include <errno.h>

/* getsockname: return loopback address. libcurl uses this to log the
   local address after connect; the actual value doesn't affect behavior. */
int getsockname(int sockfd, struct sockaddr *restrict addr,
                socklen_t *restrict addrlen) {
    (void)sockfd;
    if (!addr || !addrlen) {
        errno = EINVAL;
        return -1;
    }
    struct sockaddr_in *sin = (struct sockaddr_in *)addr;
    memset(sin, 0, sizeof(*sin));
    sin->sin_family = AF_INET;
    sin->sin_addr.s_addr = 0x0100007f; /* 127.0.0.1 in network byte order */
    sin->sin_port = 0;
    *addrlen = sizeof(*sin);
    return 0;
}

/* getpeername: return loopback address. libcurl uses this to log the
   remote address; the real address is tracked internally by libcurl. */
int getpeername(int sockfd, struct sockaddr *restrict addr,
                socklen_t *restrict addrlen) {
    (void)sockfd;
    if (!addr || !addrlen) {
        errno = EINVAL;
        return -1;
    }
    struct sockaddr_in *sin = (struct sockaddr_in *)addr;
    memset(sin, 0, sizeof(*sin));
    sin->sin_family = AF_INET;
    sin->sin_addr.s_addr = 0x0100007f; /* 127.0.0.1 */
    sin->sin_port = 0;
    *addrlen = sizeof(*sin);
    return 0;
}

#endif /* __wasi__ */
