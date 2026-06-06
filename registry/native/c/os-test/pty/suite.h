#include <sys/ioctl.h>
#include <sys/wait.h>

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

#ifdef __minix__
#undef WNOWAIT
#define getpgid(pid) (!(pid) ? getpgrp() : (errno = ENOSYS, -1))
#define tcgetsid(pid) ((void) (pid), errno = ENOSYS, -1)
#endif

#include "../misc/errors.h"
