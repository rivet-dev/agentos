/* Test whether a basic sigdelset invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	sigfillset(&set);
	if ( sigismember(&set, SIGUSR1) != 1 )
		errx(1, "control test failed");
	if ( sigdelset(&set, SIGUSR1) < 0 )
		err(1, "sigdelset");
	if ( sigismember(&set, SIGUSR1) != 0 )
		errx(1, "signal was not unset");
	if ( !sigdelset(&set, -1) )
		errx(1, "sigaddset did not fail on a negative signal");
	if ( !sigdelset(&set, 1024 * 1024) )
		errx(1, "sigaddset did not fail on a too large signal");
	return 0;
}
