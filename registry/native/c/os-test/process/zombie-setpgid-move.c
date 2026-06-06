/* Test moving a zombie process to another process group. */

#include "process.h"

int main(void)
{
	pid_t pgid = fork();
	if ( pgid < 0 )
		err(1, "fork");
	if ( !pgid )
	{
		while ( 1 )
			sleep(1);
		exit(0);
	}
	if ( setpgid(pgid, pgid) < 0 )
	{
		kill(pgid, SIGKILL);
		err(1, "setpgid on leader");
	}
	pid_t child = fork();
	if ( child < 0 )
	{
                kill(pgid, SIGKILL);
		err(1, "fork");
	}
	if ( !child )
		exit(0);
#ifdef WNOWAIT
	siginfo_t info;
	if ( waitid(P_PID, child, &info, WNOWAIT | WEXITED) < 0 )
	{
                kill(pgid, SIGKILL);
		err(1, "waitid");
	}
#else
	sleep(1);
#endif
	pid_t new_pgid = setpgid(child, pgid);
	if ( new_pgid < 0 )
	{
                kill(pgid, SIGKILL);
		err(1, "setpgid on zombie");
	}
	if ( getpgid(child) != pgid )
	{
                kill(pgid, SIGKILL);
		err(1, "getpgid(child) != leader");
	}
        kill(pgid, SIGKILL);
	int status;
	if ( waitpid(pgid, &status, 0) < 0 )
		err(1, "waitpid on leader");
	if ( waitpid(child, &status, 0) < 0 )
		err(1, "waitpid on child");
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
}
