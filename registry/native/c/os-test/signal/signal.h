#include <sys/wait.h>

#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Minix does not have its __sigaltstack14 symbol */
#ifdef __minix__
#undef sigaltstack
#define sigaltstack(a, b) ((void) (a), (void) (b), errno = ENOSYS, test_errx(1, "sigaltstack"))
#endif

/* AIX, macOS, and Minix don't have ppoll at this time */
#if defined(_AIX) || defined(__APPLE__) || defined(__minix__)
#undef ppoll
#define ppoll(a, b, c, d) ((void) (a), (void) (b), (void) (c), (void) (d), errno = ENOSYS, -1)
#endif

#include "../misc/errors.h"
