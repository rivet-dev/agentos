/* Test setpgid on a zombie process group leader. */

#include "process.h"

int main(void)
{
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		pid_t pgid = setpgid(0, 0);
		if ( pgid < 0 )
			err(1, "leader setpgid");
		exit(0);
	}
#ifdef WNOWAIT
	siginfo_t info;
	if ( waitid(P_PID, child, &info, WNOWAIT | WEXITED) < 0 )
		err(1, "waitid");
#else
	sleep(1);
#endif
	pid_t pgid = setpgid(child, child);
	if ( pgid < 0 )
		err(1, "setpgid on zombie");
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

