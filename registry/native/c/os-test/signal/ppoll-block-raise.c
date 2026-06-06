/* Test blocking SIGUSR1, raising SIGUSR1, making a pipe, and unblocking during
   ppoll. */

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
	sigset_t sigusr1;
	sigemptyset(&sigusr1);
	sigaddset(&sigusr1, SIGUSR1);
	sigprocmask(SIG_BLOCK, &sigusr1, NULL);
	sigset_t empty;
	sigemptyset(&empty);
	raise(SIGUSR1);
	int fds[2];
	if ( pipe(fds) )
		err(1, "pipe");
	struct pollfd pfd = { .fd = fds[0], .events = POLLIN };
	int ret = ppoll(&pfd, 1, NULL, &empty);
	if ( ret < 0 )
		err(1, "ppoll");
	printf("ppoll() == %i\n", ret);
	return 0;
}
