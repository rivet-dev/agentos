/* Test ignoring, blocking, raising SIGUSR1, unignoring, and unblocking. */

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
	sigset_t sigusr1;
	sigemptyset(&sigusr1);
	sigaddset(&sigusr1, SIGUSR1);
	sigprocmask(SIG_BLOCK, &sigusr1, NULL);
	raise(SIGUSR1);
	signal(SIGUSR1, handler);
	sigprocmask(SIG_UNBLOCK, &sigusr1, NULL);
	return 0;
}
