/* Test making a process group and undoing its creation. */

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
		err(1, "setpgid on child");
	if ( kill(-pgid, 0) < 0 )
		errx(1, "process was not created");
	if ( setpgid(pgid, getpgid(0)) < 0 )
		err(1, "setpgid undo");
	if ( !kill(-pgid, 0) )
		errx(1, "process group still exists");
	return 0;
}
