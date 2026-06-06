/* Test whether a basic pselect invocation works. */

#include <sys/select.h>
#include <sys/wait.h>

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
	int max = fds[0];
	fd_set read_set, error_set;
	FD_ZERO(&read_set);
	FD_ZERO(&error_set);
	FD_SET(fds[0], &read_set);
	FD_SET(fds[0], &error_set);
	struct timespec timeout = { .tv_sec = 10 };
	if ( got_signal )
		errx(1, "signal was delivered while masked");
	int ret;
	// Be interrupted by SIGCHLD and fail with EINTR.
	if ( (ret = pselect(max + 1, &read_set, NULL, &error_set, &timeout,
	                    &oldset)) < 0 )
	{
		if ( errno != EINTR )
			err(1, "pselect");
	}
	else if ( !ret )
		err(1, "pselect timeout");
	else
		errx(1, "pselect succeeded unexpectedly");
	// Check the SIGCHLD handler ran correctly.
	if ( !got_signal )
		errx(1, "SIGCHLD was not delivered");
	int status;
	waitpid(child, &status, 0);
	return 0;
}
