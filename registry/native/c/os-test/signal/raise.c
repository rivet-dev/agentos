/* Test raising SIGUSR1. */

#include "signal.h"

static void handler(int signum)
{
	(void) signum;
	int errnum = errno;
	printf("SIGUSR1\n");
	fflush(stdout);
	errno = errnum;
}

int main(void)
{
	signal(SIGUSR1, handler);
	raise(SIGUSR1);
	return 0;
}
