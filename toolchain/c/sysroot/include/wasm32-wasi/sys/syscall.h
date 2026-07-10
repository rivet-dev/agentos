#ifndef _SYS_SYSCALL_H
#define _SYS_SYSCALL_H

#ifdef __wasilibc_unmodified_upstream /* WASI has no syscall */
#include <bits/syscall.h>
#else
/* Linux syscall numbers are not part of the WASI ABI. The AgentOS libc
 * definition returns ENOSYS so upstream feature probes can take their normal
 * portable fallback. See syscall(2): https://man7.org/linux/man-pages/man2/syscall.2.html */
long syscall(long, ...);
#endif

#endif
