/* Test setpgid on a child in another session. */

#include "process.h"

int main(void)
{
	// Clean up the child when cleanup_pipe closes.
	int notify_pipe[2];
	int cleanup_pipe[2];
	if ( pipe(notify_pipe) < 0 || pipe(cleanup_pipe) < 0 )
		err(1, "pipe");
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		close(cleanup_pipe[1]);
		close(notify_pipe[0]);
		if ( setsid() < 0 )
			err(1, "setsid");
		char c = 'x';
		if ( write(notify_pipe[1], &c, 1) < 0 )
			err(1, "write");
		if ( read(cleanup_pipe[0], &c, 1) < 0 )
			err(1, "read");
		exit(0);
	}
	close(notify_pipe[1]);
	char c;
	if ( read(notify_pipe[0], &c, 1) < 0 )
		err(1, "read");
	if ( setpgid(child, child) < 0 )
		err(1, "setpgid");
	return 0;
}
