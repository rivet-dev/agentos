/* Test moving the parent process to another process group. */

#include "process.h"

int main(void)
{
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		if ( setpgid(0, 0) < 0 )
			err(1, "setpgid on child");
		if ( setpgid(getppid(), getpgid(0)) < 0 )
			err(1, "setpgid");
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
