/* Test whether a basic sigpending invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( sigprocmask(SIG_BLOCK, &set, NULL) < 0 )
		err(1, "first sigprocmask");
	if ( raise(SIGUSR1) )
		err(1, "raise");
	sigset_t pending;
	if ( sigpending(&pending) < 0 )
		err(1, "sigpending");
	if ( sigismember(&pending, SIGUSR1) != 1 )
		errx(1, "SIGUSR1 was not pending");
	return 0;
}
