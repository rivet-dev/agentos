/* Test blocking SIGUSR1, raising SIGUSR1, making a pipe, writing to the pipe,
   and unblocking during ppoll. */

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
	// Signal is supposed to be delivered when ppoll replaces the signal mask
	// before iterating the descriptors per POSIX, even if the condition waited
	// for is already true.
	if ( write(fds[1], "x", 1) < 0 )
		err(1, "write");
	struct pollfd pfd = { .fd = fds[0], .events = POLLIN };
	// POSIX requires EINTR or returning the pending events.
	int ret = ppoll(&pfd, 1, NULL, &empty);
	if ( ret < 0 )
		err(1, "ppoll");
	if ( !ret )
	{
		printf("ppoll() == 0\n");
		return 0;
	}
	printf("0");
	if ( pfd.revents & POLLIN )
		printf(" | POLLIN");
	if ( pfd.revents & POLLOUT )
		printf(" | POLLOUT");
	if ( pfd.revents & POLLERR )
		printf(" | POLLERR");
	if ( pfd.revents & POLLHUP )
		printf(" | POLLHUP");
	if ( pfd.revents & POLLNVAL )
		printf(" | POLLNVAL");
	printf("\n");
	return 0;
}
