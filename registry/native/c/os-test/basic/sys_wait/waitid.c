/* Test whether a basic waitid invocation works. */

#include <sys/wait.h>

#include <errno.h>
#include <signal.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	// Put ourselves in our own process group, so one child 3 inherits it.
	if ( setpgid(0, 0) < 0 )
		errx(1, "self setpgid");

	// Children 0, 1, and 2 have their own process group and child 3 is in ours.
	pid_t children[4];
	for ( int i = 0; i < 4; i++ )
	{
		if ( (children[i] = fork() < 0) )
			err(1, "first fork");
		if ( !children[i] )
		{
			if ( i <= 2 )
				setpgid(0, 0);
			_exit(0);
		}
		if ( i <= 2 )
			setpgid(children[i], children[i]);
	}

	siginfo_t info;

	// Collect child 0 (its own pgid) by pid.
	if ( waitid(P_PID, children[0], &info, 0) < 0 )
		err(1, "child 0 waitid");
	if ( info.si_pid != children[0] )
		errx(1, "child 0 waitid gave wrong child");
	if ( !WIFEXITED(info.si_status) || WEXITSTATUS(info.si_status) != 0 )
		errx(1, "child 0 did not exit 0");
	// POSIX requires that info.si_status 0 means exit 0.
	if ( info.si_status != 0 )
		errx(1, "child 0 had non-zero info.si_status");
	// POSIX requires that si_signo is SIGCHLD.
	if ( info.si_signo != SIGCHLD )
		errx(1, "child 0 si_signo != SIGCHLD");

	// Collect child 3 (our pgid) by our own pgid.
	if ( waitid(P_PGID, getpgid(0), &info, 0) < 0 )
		err(1, "child 3 waitid");
	if ( info.si_pid != children[3] )
		errx(1, "child 3 waitid gave wrong child");
	if ( !WIFEXITED(info.si_status) || WEXITSTATUS(info.si_status) != 0 )
		errx(1, "child 3 did not exit 0");
	if ( info.si_status != 0 )
		errx(1, "child 3 had non-zero info.si_status");
	if ( info.si_signo != SIGCHLD )
		errx(1, "child 3 si_signo != SIGCHLD");

	// Collect child 2 (its own pgid) by its own pgid.
	if ( waitid(P_PGID, children[2], &info, 0) < 0 )
		err(1, "child 2 waitid");
	if ( info.si_pid != children[2] )
		errx(1, "child 2 waitid gave wrong child");
	if ( !WIFEXITED(info.si_status) || WEXITSTATUS(info.si_status) != 0 )
		errx(1, "child 2 did not exit 0");
	if ( info.si_status != 0 )
		errx(1, "child 2 had non-zero info.si_status");
	if ( info.si_signo != SIGCHLD )
		errx(1, "child 2 si_signo != SIGCHLD");

	// Collect child 1 (its own pgid) by asking for any child.
	if ( waitid(P_ALL, 0, &info, 0) < 0 )
		err(1, "child 1 waitid");
	if ( info.si_pid != children[1] )
		errx(1, "child 1 waitid gave wrong child");
	if ( !WIFEXITED(info.si_status) || WEXITSTATUS(info.si_status) != 0 )
		errx(1, "child 1 did not exit 0");
	if ( info.si_status != 0 )
		errx(1, "child 1 had non-zero info.si_status");
	if ( info.si_signo != SIGCHLD )
		errx(1, "child 1 si_signo != SIGCHLD");

	// Test failing if there are no more children.
	if ( waitid(P_ALL, 0, &info, 0) < 0 )
	{
		if ( errno != ECHILD )
			err(1, "fifth waitid");
	}
	else
		errx(1, "fifth waitid succeeded with no children");

	return 0;
}
