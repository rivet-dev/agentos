/* Test what is the process group of a zombie process? */

#include "process.h"

int main(void)
{
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
		exit(0);
#ifdef WNOWAIT
	siginfo_t info;
	if ( waitid(P_PID, child, &info, WNOWAIT | WEXITED) < 0 )
		err(1, "waitid");
#else
	sleep(1);
#endif
	pid_t pgid = getpgid(child);
	if ( pgid < 0 )
		err(1, "getpgid on zombie");
	if ( pgid != getpgid(0) )
		err(1, "getpgid(child) != getpgid(parent)");
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
