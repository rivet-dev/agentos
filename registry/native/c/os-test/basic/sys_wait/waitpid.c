/* Test whether a basic waitpid invocation works. */

#include <sys/wait.h>

#include <errno.h>
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
		if ( (children[i] = fork()) < 0 )
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

	int status;
	pid_t pid;

	// Collect child 0 (its own pgid) by pid.
	if ( (pid = waitpid(children[0], &status, 0)) < 0 )
		err(1, "child 0 waitpid");
	if ( pid != children[0] )
		errx(1, "child 0 waitpid gave wrong child");
	if ( !WIFEXITED(status) || WEXITSTATUS(status) != 0 )
		errx(1, "child 0 did not exit 0");
	// POSIX requires that status 0 means exit 0.
	if ( status != 0 )
		errx(1, "child 0 had non-zero status");

	// Collect child 3 (our pgid) by our own pgid.
	if ( (pid = waitpid(0, &status, 0)) < 0 )
		err(1, "child 3 waitpid");
	if ( pid != children[3] )
		errx(1, "child 3 waitpid gave wrong child");
	if ( !WIFEXITED(status) || WEXITSTATUS(status) != 0 )
		errx(1, "child 3 did not exit 0");
	if ( status != 0 )
		errx(1, "child 3 had non-zero status");

	// Collect child 2 (its own pgid) by its own pgid.
	if ( (pid = waitpid(-children[2], &status, 0)) < 0 )
		err(1, "child 2 waitpid");
	if ( pid != children[2] )
		errx(1, "child 2 waitpid gave wrong child");
	if ( !WIFEXITED(status) || WEXITSTATUS(status) != 0 )
		errx(1, "child 2 did not exit 0");
	if ( status != 0 )
		errx(1, "child 2 had non-zero status");

	// Collect child 1 (its own pgid) by asking for any child.
	if ( (pid = waitpid(-1, &status, 0)) < 0 )
		err(1, "child 1 waitpid");
	if ( pid != children[1] )
		errx(1, "child 1 waitpid gave wrong child");
	if ( !WIFEXITED(status) || WEXITSTATUS(status) != 0 )
		errx(1, "child 1 did not exit 0");
	if ( status != 0 )
		errx(1, "child 1 had non-zero status");

	// Test failing if there are no more children.
	if ( (pid = waitpid(-1, &status, 0)) < 0 )
	{
		if ( errno != ECHILD )
			err(1, "fifth waitpid");
	}
	else
		errx(1, "fifth waitpid succeeded with no children");

	return 0;
}
