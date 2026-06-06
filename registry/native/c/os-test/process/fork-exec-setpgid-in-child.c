/* Test exec in the child process and then setpgid on the child in the child. */

#include "process.h"

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( setpgid(0, 0) < 0 )
			err(1, "setpgid");
		return 0;
	}
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		execlp(argv[0], argv[0], "1", (char*) NULL);
		err(1, "%s", argv[0]);
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
