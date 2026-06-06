#ifdef __HAIKU__
#define _BSD_SOURCE
#endif

#include <sys/wait.h>

#include <errno.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifdef __minix__
#undef WNOWAIT
#define getpgid(pid) (!(pid) ? getpgrp() : (errno = ENOSYS, -1))
#endif

#include "../misc/errors.h"
