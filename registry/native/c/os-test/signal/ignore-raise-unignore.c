/* Test ignoring, raising SIGUSR1, and unignoring. */

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
	signal(SIGUSR1, SIG_IGN);
	raise(SIGUSR1);
	signal(SIGUSR1, handler);
	return 0;
}
