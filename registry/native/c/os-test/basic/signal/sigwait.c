/* Test whether a basic sigwait invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( sigprocmask(SIG_BLOCK, &set, NULL) < 0 )
		err(1, "sigprocmask");
	raise(SIGUSR1);
	int signo;
	if ( (errno = sigwait(&set, &signo)) < 0 )
		err(1, "sigwait");
	return 0;
}
