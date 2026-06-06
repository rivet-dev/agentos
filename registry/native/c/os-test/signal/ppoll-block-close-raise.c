/* Test blocking SIGUSR1, raising SIGUSR1, providing a bad file descriptor to
   poll, and unblocking during ppoll. */

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
	int fds[2];
	if ( pipe(fds) )
		err(1, "pipe");
	close(fds[0]);
	// Signal is supposed to be delivered when ppoll replaces the signal mask
	// before iterating the descriptors per POSIX, even if the descriptor is
	// invalid and the POLLNVAL event is immediately true.
	raise(SIGUSR1);
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
