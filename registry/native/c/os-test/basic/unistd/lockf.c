/*[XSI]*/
/* Test whether a basic lockf invocation works. */

#include <sys/wait.h>

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	int fd = fileno(fp);
	if ( lockf(fd, F_LOCK, 0) < 0 )
		err(1, "first lockf");
	if ( lockf(fd, F_LOCK, 0) < 0 )
		err(1, "second lockf");
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		if ( lockf(fd, F_TLOCK, 0) < 0 )
		{
			if ( errno != EACCES && errno != EAGAIN )
				err(1, "child lockf");
		}
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
