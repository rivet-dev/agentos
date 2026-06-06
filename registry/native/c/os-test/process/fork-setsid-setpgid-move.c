/* Test moving a session leader to another process group in the session. */

#include "process.h"

int main(void)
{
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		if ( setsid() < 0 )
			err(1, "setsid");
		// Clean up children once pipe is closed.
		int pipes[2];
		if ( pipe(pipes) < 0 )
			err(1, "pipe");
		pid_t pgid = fork();
		if ( pgid < 0 )
			err(1, "fork");
		char c;
		if ( !pgid )
		{
			close(pipes[1]);
			read(pipes[0], &c, 1);
			exit(0);
		}
		if ( setpgid(pgid, pgid) < 0 )
			err(1, "setpgid on child");
		if ( setpgid(getpid(), pgid) < 0 )
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
