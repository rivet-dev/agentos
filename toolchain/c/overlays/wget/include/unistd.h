#ifndef AGENTOS_WGET_UNISTD_OVERLAY_H
#define AGENTOS_WGET_UNISTD_OVERLAY_H

#include_next <unistd.h>

/*
 * GNU Wget's ptimer fallback only needs to know that Linux-style POSIX timer
 * IDs are not compile-time portable here. The underlying sysroot still exposes
 * WASI clocks for libc++ and other consumers that use the WASI clockid_t ABI.
 */
#undef _POSIX_TIMERS
#define _POSIX_TIMERS 0

#endif
