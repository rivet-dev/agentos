/* Test blocking, raising SIGCHLD, and unblocking. */

#include "signal.h"

static void handler(int signum)
{
	(void) signum;
	int errnum = errno;
	printf("SIGCHLD\n");
	fflush(stdout);
	errno = errnum;
}

int main(void)
{
	sigset_t sigchld;
	sigemptyset(&sigchld);
	sigaddset(&sigchld, SIGCHLD);
	sigprocmask(SIG_BLOCK, &sigchld, NULL);
	signal(SIGCHLD, handler);
	raise(SIGCHLD);
	sigprocmask(SIG_UNBLOCK, &sigchld, NULL);
	return 0;
}
