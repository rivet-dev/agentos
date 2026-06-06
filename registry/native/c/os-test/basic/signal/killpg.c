/*[XSI]*/
/* Test whether a basic killpg invocation works. */

#include <sys/wait.h>

#include <signal.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	pid_t child1 = fork();
	if ( child1 < 0 )
		err(1, "fork");
	if ( !child1 )
	{
		close(fds[1]);
		char c;
		read(fds[0], &c, 1);
		return 0;
	}
	pid_t child2 = fork();
	if ( child2 < 0 )
		err(1, "fork");
	if ( !child2 )
	{
		close(fds[1]);
		char c;
		read(fds[0], &c, 1);
		return 0;
	}
	if ( setpgid(child1, child1) < 0 ||
	     setpgid(child2, child1) < 0 )
	{
		warn("setpgid");
		kill(child1, SIGKILL);
		kill(child2, SIGKILL);
		exit(1);
	}
	if ( killpg(child1, SIGUSR1) < 0 )
		err(1, "kill");
	int status;
	if ( waitpid(child1, &status, 0) < 0 )
		err(1, "waitpid child1");
	if ( !WIFSIGNALED(status) || WTERMSIG(status) != SIGUSR1 )
		err(1, "child1 process was not terminated by SIGUSR1");
	if ( waitpid(child2, &status, 0) < 0 )
		err(1, "waitpid child2");
	if ( !WIFSIGNALED(status) || WTERMSIG(status) != SIGUSR1 )
		err(1, "child2 process was not terminated by SIGUSR1");
	return 0;
}
