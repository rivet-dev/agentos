/* Test exec in the child process and then setpgid on the child in the
   parent. */

#include "process.h"

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		char c = 'x';
		if ( write(1, &c, 1) < 0 )
			err(1, "write");
		if ( read(0, &c, 1) < 0 )
			err(1, "read");
		return 0;
	}
	// Clean up the child when end_pipe closes.
	int exec_pipe[2];
	int end_pipe[2];
	if ( pipe(exec_pipe) < 0 || pipe(end_pipe) < 0 )
		err(1, "pipe");
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		close(exec_pipe[0]);
		close(end_pipe[1]);
		dup2(exec_pipe[1], 1);
		dup2(end_pipe[0], 0);
		execlp(argv[0], argv[0], "1", (char*) NULL);
		err(1, "%s", argv[0]);
	}
	close(exec_pipe[1]);
	close(end_pipe[0]);
	char c;
	if ( read(exec_pipe[0], &c, 1) < 0 )
		err(1, "read");
	if ( setpgid(child, child) < 0 )
		err(1, "setpgid");
	return 0;
}
