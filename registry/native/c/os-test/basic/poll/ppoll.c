/* Test whether a basic ppoll invocation works. */

#include <sys/wait.h>

#include <poll.h>
#include <signal.h>
#include <unistd.h>

#include "../basic.h"

static volatile sig_atomic_t got_signal;

static void on_signal(int signo)
{
	got_signal = signo;
}

int main(void)
{
	// See that ppoll unblocks SIGCHLD and wakes on a child exit.
	// Block SIGCHLD and handle it without restarting the syscalll.
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGCHLD);
	if ( sigprocmask(SIG_BLOCK, &set, &oldset) < 0 )
		err(1, "sigprocmask");
	struct sigaction sa = { .sa_handler = on_signal };
	if ( sigaction(SIGCHLD, &sa, NULL) < 0 )
		err(1, "sigaction");
	// Make a child that exits immediately so SIGCHLD is pending but not
	// delivered because it's blocked.
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
		_exit(0);
	// Nothing will happen on this pipe.
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	struct pollfd pfds[1] =
	{
		{ .fd = fds[0], .events = POLLIN },
	};
	struct timespec timeout = { .tv_sec = 10 };
	if ( got_signal )
		errx(1, "signal was delivered while masked");
	int ret;
	// Be interrupted by SIGCHLD and fail with EINTR.
	if ( (ret = ppoll(pfds, 1, &timeout, &oldset)) < 0 )
	{
		if ( errno != EINTR )
			err(1, "ppoll");
	}
	else if ( !ret )
		err(1, "ppoll timeout");
	else
		errx(1, "ppoll succeeded unexpectedly");
	// Check the SIGCHLD handler ran correctly.
	if ( !got_signal )
		errx(1, "SIGCHLD was not delivered");
	int status;
	waitpid(child, &status, 0);
	return 0;
}
