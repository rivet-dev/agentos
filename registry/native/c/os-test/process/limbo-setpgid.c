/* Test making a process in limbo (process group leader that has been awaited
   but still has a member) into a process group leader. */

#include "process.h"

int main(void)
{
	// Clean up children once pipe is closed.
	int pipes[2];
	if ( pipe(pipes) < 0 )
		err(1, "pipe");
	pid_t zombie = fork();
	if ( zombie < 0 )
		err(1, "fork");
	char c;
	if ( !zombie )
	{
		close(pipes[1]);
		read(pipes[0], &c, 1);
		exit(0);
	}
	if ( setpgid(zombie, zombie) < 0 )
 		err(1, "setpgid on zombie");
	pid_t member = fork();
	if ( member < 0 )
		err(1, "fork");
	if ( !member )
	{
		close(pipes[1]);
		read(pipes[0], &c, 1);
		exit(0);
	}
	if ( setpgid(member, zombie) < 0 )
		err(1, "setpgid on member");
	if ( kill(zombie, SIGKILL) < 0 )
		err(1, "kill zombie");
	int status;
	waitpid(zombie, &status, 0);
	pid_t pgid = setpgid(zombie, zombie);
	if ( pgid < 0 )
		err(1, "setpgid on zombie");
	close(pipes[1]);
	waitpid(member, &status, 0);
	return 0;
}
