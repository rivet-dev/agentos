/* Test whether a basic kill invocation works. */

#include <sys/wait.h>

#include <signal.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		close(fds[1]);
		char c;
		read(fds[0], &c, 1);
		return 0;
	}
	if ( kill(child, SIGUSR1) < 0 )
		err(1, "kill");
	int status;
	if ( waitpid(child, &status, 0) < 0 )
		err(1, "waitpid");
	if ( !WIFSIGNALED(status) || WTERMSIG(status) != SIGUSR1 )
		err(1, "child process was not terminated by SIGUSR1");
	return 0;
}
