/* Test setpgid on a child process. */

#include "process.h"

int main(void)
{
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		if ( setpgid(0, 0) < 0 )
			err(1, "setpgid");
		if ( getpid() != getpgid(0) )
			errx(1, "getpid() != getpgid(0) (%li != %li)", (long) getpid(), (long) getpgid(0));
		exit(0);
	}
	int status;
	if ( waitpid(child, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
}
