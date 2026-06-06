/*[XSI]*/
/* Test whether a basic getpriority invocation works. */

#include <sys/resource.h>

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( getpriority(PRIO_PROCESS, getpid()) == -1 && errno )
		err(1, "getpriority: PRIO_PROCESS");
	errno = 0;
	if ( getpriority(PRIO_PGRP, getpgid(0)) == -1 && errno )
		err(1, "getpriority: PRIO_PGRP");
	errno = 0;
	if ( getpriority(PRIO_USER, getuid()) == -1 && errno )
		err(1, "getpriority: PRIO_USER");
	return 0;
}
