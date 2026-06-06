/* Test blocking SIGUSR1, raising SIGUSR1, making a pipe, unblocking during
   ppoll, after which a child process writes to the pipe and sends SIGUSR1. */

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
	pid_t parent = getpid();
	pid_t pid = fork();
	if ( pid < 0 )
		err(1, "fork");
	if ( !pid )
	{
		// Race condition since we can't observe if ppoll is truly waiting.
		usleep(100 * 1000);
		// Sortix does not implement SIGSTOP yet, so just race instead.
#ifndef __sortix__
		kill(parent, SIGSTOP);
		// Ensure the SIGSTOP signal has been dispatched.
		usleep(100 * 1000);
#endif
		// The pipe has input by the time SIGCONT has happened, so ppoll is
		// allowed to return the event plus the signal.
		if ( write(fds[1], "x", 1) < 0 )
			err(1, "write");
		// SIGUSR1 has happened by the time SIGCONT has happened so the ppoll
		// call must dispatch it when resumed.
		kill(parent, SIGUSR1);
#ifndef __sortix__
		kill(parent, SIGCONT);
#endif
		_Exit(0);
	}
	struct pollfd pfd = { .fd = fds[0], .events = POLLIN };
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
