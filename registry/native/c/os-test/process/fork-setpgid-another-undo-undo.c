/* Test making a process group with two members and undoing its creation. */

#include "process.h"

int main(void)
{
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
	if ( !kill(-pgid, 0) )
		errx(1, "process group already existed");
	if ( setpgid(pgid, pgid) < 0 )
		err(1, "setpgid on leader");
	if ( kill(-pgid, 0) < 0 )
		errx(1, "process was not created");
	pid_t member = fork();
	if ( member < 0 )
		err(1, "fork");
	if ( !member )
	{
		close(pipes[1]);
		read(pipes[0], &c, 1);
		exit(0);
	}
	if ( setpgid(member, pgid) < 0 )
		err(1, "setpgid on member");
	if ( setpgid(pgid, getpgid(0)) < 0 )
		err(1, "setpgid undo leader");
	if ( setpgid(member, getpgid(0)) < 0 )
		err(1, "setpgid undo member");
	if ( !kill(-pgid, 0) )
		errx(1, "process group still exists");
	return 0;
}
