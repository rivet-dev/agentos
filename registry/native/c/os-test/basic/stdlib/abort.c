/* Test whether a basic abort invocation works. */

#include <sys/resource.h>
#include <sys/wait.h>

#include <signal.h>
#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		struct rlimit limit;
		limit.rlim_cur = 0;
		limit.rlim_max = 0;
		if ( setrlimit(RLIMIT_CORE, &limit) < 0 )
			errx(1, "setrlimit(RLIMIT_CORE, {0, 0})");
		abort();
		return 0;
	}
#ifdef __HAIKU__
	alarm(1); // abort gets stuck on Haiku for some reason.
#endif
	int status;
	if ( waitpid(child, &status, 0) < 0 )
		err(1, "waitpid");
	if ( !WIFSIGNALED(status) || WTERMSIG(status) != SIGABRT )
		errx(1, "abort did not cause SIGABRT");
	return 0;
}
