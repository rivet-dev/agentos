/* Test if waitpid on a process group. */

#include "process.h"

int main(void)
{
	int alive_fd[2], pgid_fd[2];
	if ( pipe(alive_fd) < 0 || pipe(pgid_fd) < 0 )
		err(1, "pipe");
	// Make a process group that is not a direct child so waitpid can't see it.
	pid_t pgid = fork();
	if ( pgid < 0 )
		err(1, "fork");
	if ( !pgid )
	{
		close(alive_fd[1]);
		close(pgid_fd[0]);
		// Double fork to not be a direct child.
		pgid = fork();
		if ( pgid < 0 )
			err(1, "fork");
		if ( pgid )
			_exit(0);
		// And become a process group leader.
		if ( setpgid(0, 0) < 0 )
			err(1, "setpgid");
		// Tell the original process what our process group id is.
		pgid = getpgid(0);
		if ( write(pgid_fd[1], &pgid, sizeof(pgid)) < (ssize_t) sizeof(pgid) )
			err(1, "write");
		close(pgid_fd[1]);
		// Stay alive as long as the alive pipe is connected to the original
		// process, so we exit when it does. It will never write here.
		char c;
		read(alive_fd[0], &c, 1);
		_exit(0);
	}
	// Wait for the process group to exist and receive it's id.
	if ( read(pgid_fd[0], &pgid, sizeof(pgid)) < (ssize_t) sizeof(pgid) )
		err(1, "read");
	// Make a child that is in the process group.
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		// Race in the parent/child to place the child in the process group,
		// so there isn't any doubt whether the child joined the group before
		// becoming a zombie, which could possibly affect results on buggy
		// systems. This is a control test after all.
		if ( setpgid(0, pgid) < 0 )
			err(1, "setpgid of child into pgid");
		_exit(0);
	}
	// Place the child inside the indirect process group.
	if ( setpgid(child, pgid) < 0 )
		err(1, "setpgid of child into pgid");
	// Time out if the process hangs inside waitpid.
	alarm(2);
	int status;
	// waitpid is supposed to return the child here.
	pid_t result = waitpid(-pgid, &status, 0);
	if ( result < 0 )
		err(1, "waitpid");
	else if ( result == 0 )
		errx(1, "waitpid() == 0");
	else if ( result == child )
		return 0;
	else
		errx(1, "waitpid returned strange child");
}
