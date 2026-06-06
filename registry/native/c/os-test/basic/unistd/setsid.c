/* Test whether a basic setsid invocation works. */

#include <sys/wait.h>

#include <unistd.h>

#include "../basic.h"

int main(void)
{
    pid_t child = fork();
    if ( child < 0 )
            err(1, "fork");
    if ( !child )
    {
		if ( setsid() < 0 )
			err(1, "setsid");
		if ( getsid(0) != getpid() )
			errx(1, "getsid(0) != getpid()");
		if ( getpgid(0) != getpid() )
			errx(1, "getpgid(0) != getpid()");
		return 0;
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
	return 0;

}
